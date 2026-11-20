use super::*;
use solidcraft_geom::{Aabb3, Mesh, MeshGeom};

fn fixed_request_metadata_bytes() -> u64 {
    request().retained_resource_metadata_bytes()
}

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
        let expected = admit(&empty(), &request).unwrap().estimated_total_bytes + request.retained_resource_metadata_bytes();
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
        assert_eq!(host.admitted_bytes(), retained + frame._reservation.bytes + fixed_request_metadata_bytes());
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
    fn two_pass_target_retains_one_snapshot_slot_without_blocking_final() {
        let (host, job, mut running) = fixture(2);
        let credits = running.checkpoints.as_ref().unwrap();
        assert_eq!(credits.capacity, credits.slot_bytes + FRAME_METADATA_BYTES);
        before_preview(&mut running);
        assert!(running.advance(1).unwrap());
        let preview = job.preview().unwrap().unwrap();
        assert_eq!(preview.info.frame_sequence, 1);
        assert_eq!(preview.info.samples_per_pixel, 1);
        let before = preview.png().to_vec();
        while running.advance(1).unwrap() {}
        assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
        assert_eq!(job.status().unwrap().preview_retention_skips, 0);
        assert!(job.preview().unwrap().is_none());
        let frame = job.frame().unwrap().unwrap();
        assert_eq!(frame.info.job, job.id());
        assert_eq!(preview.png(), before, "the sole retained checkpoint remains immutable");
        assert_eq!(host.admitted_bytes(), frame._reservation.bytes + preview._reservation.bytes + fixed_request_metadata_bytes());
        drop(running);
        drop(job);
        drop(frame);
        assert_eq!(host.admitted_bytes(), preview._reservation.bytes);
        drop(preview);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[test]
    fn checkpoint_memory_is_independent_of_target_after_two_prepaid_slots() {
        let mut estimated = Vec::new();
        for samples in [3, 8, 64] {
            let (host, job, running) = fixture(samples);
            estimated.push(host.admitted_bytes());
            assert_eq!(running.checkpoints.as_ref().unwrap().capacity, 2 * running.checkpoints.as_ref().unwrap().slot_bytes + FRAME_METADATA_BYTES);
            drop(running);
            drop(job);
            assert_eq!(host.admitted_bytes(), 0);
        }
        assert!(estimated.windows(2).all(|pair| pair[0] == pair[1]));
        assert!(std::mem::size_of::<CheckpointCredits>() + 32 <= FRAME_METADATA_BYTES as usize);
    }

    #[test]
    fn every_pass_reuses_two_slots_without_consumer_acknowledgement() {
        let (host, job, mut running) = fixture(8);
        let initial = host.admitted_bytes();
        let mut prefixes = Vec::new();
        for samples in 1..8 {
            before_preview(&mut running);
            assert!(running.advance(1).unwrap());
            let preview = job.preview().unwrap().unwrap();
            assert_eq!(preview.info.samples_per_pixel, samples);
            assert_eq!(preview.info.frame_sequence, samples);
            assert_eq!(preview.info.frame.job, job.id());
            assert_eq!(preview.info.frame.scene_generation, 19);
            assert_eq!(preview.info.frame.png_sha256, format!("{:x}", Sha256::digest(preview.png())));
            prefixes.push(preview.png().to_vec());
            assert_eq!(host.admitted_bytes(), initial);
            assert_eq!(job.status().unwrap().preview_retention_skips, 0);
            // This temporary consumer drops its Arc; the host's old snapshot is
            // reclaimed by the next publication without an acknowledgement API.
        }
        while running.advance(1).unwrap() {}
        let frame = job.frame().unwrap().unwrap();
        assert_eq!(frame.info.job, job.id());
        assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
        assert_eq!(host.admitted_bytes(), frame._reservation.bytes + fixed_request_metadata_bytes());
        assert_eq!(prefixes.len(), 7);
        drop(running);
        drop(job);
        drop(frame);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[test]
    fn pinned_snapshots_skip_publication_then_resume_without_stalling_final_work() {
        let (host, job, mut running) = fixture(8);
        let initial = host.admitted_bytes();
        before_preview(&mut running);
        assert!(running.advance(1).unwrap());
        let first = job.preview().unwrap().unwrap();
        let first_bytes = first.png().to_vec();
        before_preview(&mut running);
        assert!(running.advance(1).unwrap());
        let second = job.preview().unwrap().unwrap();
        let second_bytes = second.png().to_vec();
        before_preview(&mut running);
        assert_eq!(running.pending_png.as_ref().unwrap().preview_samples, Some(3));
        assert!(running.advance(1).unwrap());
        assert_eq!(job.status().unwrap().preview_retention_skips, 1);
        let retained = job.preview().unwrap().expect("a skipped publication must keep the latest published preview available");
        assert!(Arc::ptr_eq(&retained, &second), "a pinned latest preview must retain its existing allocation and lease");
        let status = job.status().unwrap();
        let preview = status.preview.expect("status must still identify the latest published complete pass");
        assert_eq!(preview.frame_sequence, 2);
        assert_eq!(preview.samples_per_pixel, 2);
        assert_eq!(preview.frame.png_sha256, second.info.frame.png_sha256);
        drop(retained);
        assert!(job.frame().unwrap().is_none());
        assert_eq!(host.admitted_bytes(), initial);
        assert_eq!(first.png(), first_bytes);
        assert_eq!(second.png(), second_bytes);
        drop(first);
        for samples in 4..8 {
            before_preview(&mut running);
            assert!(running.advance(1).unwrap());
            let preview = job.preview().unwrap().unwrap();
            assert_eq!(preview.info.samples_per_pixel, samples);
            assert_eq!(preview.info.frame_sequence, samples - 1, "sequence counts real publications");
            assert_eq!(host.admitted_bytes(), initial);
        }
        while running.advance(1).unwrap() {}
        assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
        assert_eq!(job.status().unwrap().preview_retention_skips, 1);
        assert!(job.preview().unwrap().is_none());
        assert_eq!(second.png(), second_bytes, "old snapshots remain immutable through final publication");
        let frame = job.frame().unwrap().unwrap();
        assert_eq!(host.admitted_bytes(), frame._reservation.bytes + second._reservation.bytes + fixed_request_metadata_bytes());
        drop(running);
        drop(job);
        drop(frame);
        assert_eq!(host.admitted_bytes(), second._reservation.bytes);
        drop(second);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[test]
    fn retaining_both_slots_never_prevents_completion() {
        let (host, job, mut running) = fixture(8);
        before_preview(&mut running);
        assert!(running.advance(1).unwrap());
        let first = job.preview().unwrap().unwrap();
        before_preview(&mut running);
        assert!(running.advance(1).unwrap());
        let second = job.preview().unwrap().unwrap();
        while running.advance(1).unwrap() {}
        assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
        assert_eq!(job.status().unwrap().preview_retention_skips, 5);
        assert_eq!(first.info.samples_per_pixel, 1);
        assert_eq!(second.info.samples_per_pixel, 2);
        drop(running);
        drop(job);
        drop(first);
        drop(second);
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
fn native_raytrace_refraction_capability_resolves_preview_and_final_images() {
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
    let final_image = crate::resolve_request(std::slice::from_ref(&backend), &request).unwrap();
    assert_eq!(final_image.executor, resolved.executor);
    assert_eq!(final_image.profile.mode, RenderMode::Final);
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
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(a);
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
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(job);
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
    assert_eq!(budget.used.load(Ordering::Acquire), 2 * fixed_request_metadata_bytes());
    drop(active);
    drop(queued);
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
    let duration = job.completed_duration().unwrap().unwrap();
    assert_eq!(job.completed_duration().unwrap(), Some(duration), "completed duration is independent of status polling");
}

#[test]
fn completed_duration_is_captured_at_publication_and_never_for_cancelled_work() {
    let host = CpuHost::cooperative();
    let job = host.submit(id(1, 1), 12, empty(), request()).unwrap();
    assert!(job.completed_duration().unwrap().is_none());
    drain(&host);
    let duration = job.completed_duration().unwrap().unwrap();
    assert!(duration.as_secs_f64().is_finite());
    job.status().unwrap();
    host.pump(1).unwrap();
    assert_eq!(job.completed_duration().unwrap(), Some(duration));
    let cancelled = host.submit(id(1, 2), 12, empty(), request()).unwrap();
    cancelled.cancel().unwrap();
    drain(&host);
    assert!(cancelled.completed_duration().unwrap().is_none());
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
    assert_eq!(host.admitted_bytes(), frame.png.capacity() as u64 + FRAME_METADATA_BYTES + fixed_request_metadata_bytes());
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
        assert_eq!(budget.used.load(Ordering::Acquire), fixed_request_metadata_bytes());
        drop(job);
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
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(job);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn png_and_exr_reuse_one_sha_state_without_mixing_chunk_boundaries() {
    let png: Vec<u8> = (0..PNG_HASH_CHUNK_BYTES + 7).map(|i| (i % 251) as u8).collect();
    let exr: Vec<u8> = (0..PNG_HASH_CHUNK_BYTES * 2 + 19).map(|i| (i % 239) as u8).collect();
    let mask: Vec<u8> = (0..PNG_HASH_CHUNK_BYTES * 3 + 31).map(|i| (i % 233) as u8).collect();
    for mask in [None, Some(mask)] {
        let mut pending = PendingPng::new(png.clone());
        pending.exr = Some(exr.clone());
        pending.denoise_mask = mask.clone();
        let cancel = CancelToken::default();
        assert!(!pending.step(1, &cancel).unwrap());
        assert!(pending.png_digest.is_none());
        assert!(!pending.step(1, &cancel).unwrap());
        assert_eq!(pending.png_digest.as_ref().unwrap(), &Sha256::digest(&png));
        assert_eq!(pending.exr_hashed, 0);
        while !pending.step(1, &cancel).unwrap() {}
        assert_eq!(pending.png_digest.as_ref().unwrap(), &Sha256::digest(&png));
        assert_eq!(pending.exr_digest.as_ref().unwrap(), &Sha256::digest(&exr));
        assert_eq!(pending.denoise_mask_digest, mask.as_ref().map(Sha256::digest));
        assert_eq!(pending.hashed, png.len());
        assert_eq!(pending.exr_hashed, exr.len());
        assert_eq!(pending.denoise_mask_hashed, mask.as_ref().map_or(0, Vec::len));
    }
    let metadata = std::mem::size_of::<Running>()
        + std::mem::size_of::<RenderFrame>()
        + std::mem::size_of::<RenderPreview>()
        + PREPARED_PROOF_HEADER_BYTES
        + 3 * 64
        + 2 * crate::MAX_RENDER_ID_BYTES
        + 32;
    assert!(metadata <= FRAME_METADATA_BYTES as usize);
    eprintln!(
        "fixed completion metadata: Running {}, final {}, preview {}, combined {metadata}/{}",
        std::mem::size_of::<Running>(),
        std::mem::size_of::<RenderFrame>(),
        std::mem::size_of::<RenderPreview>(),
        FRAME_METADATA_BYTES
    );
}

#[test]
fn png_digest_metadata_fits_the_existing_allowance_without_changing_admission() {
    // Fixed digest/offset state and the completed frame (including its bounded
    // hex string and backend/executor IDs) fit the shared metadata allowance.
    assert!(std::mem::size_of::<PendingPng>() <= 512);
    for linear_master in [false, true] {
        let metadata = std::mem::size_of::<Running>()
            + std::mem::size_of::<RenderFrame>()
            + PREPARED_PROOF_HEADER_BYTES
            + 64
            + u32::from(linear_master) as usize * 64
            + 2 * crate::MAX_RENDER_ID_BYTES
            + 32;
        assert!(
            metadata <= FRAME_METADATA_BYTES as usize,
            "PNG/EXR completion metadata {metadata} exceeds {} (Running {}, Frame {}, linear_master {linear_master})",
            FRAME_METADATA_BYTES,
            std::mem::size_of::<Running>(),
            std::mem::size_of::<RenderFrame>()
        );
    }
    let host = CpuHost::cooperative();
    let expected = admit(&empty(), &request()).unwrap().estimated_total_bytes + fixed_request_metadata_bytes();
    let job = host.submit(id(70, 1), 1, empty(), request()).unwrap();
    assert_eq!(host.admitted_bytes(), expected);
    assert!(job.status().unwrap().frame.is_none());
    job.cancel().unwrap();
    drain(&host);
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(job);
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
            caustic_work: None,
            cpu_workers: None,
            host_memory: None,
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
fn unqualified_executors_refuse_glass_models_and_caustics_before_acquiring_a_lease() {
    let (host, _) = probe_host(1024);
    let options = JobOptions { caustics: Some(Default::default()), ..Default::default() };
    let error = host.admit(&empty(), &request(), options).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidRequest);
    assert!(error.message.contains("caustic photons"));
    assert_eq!(host.admitted_bytes(), 0);
    let mut scene = empty();
    scene.meshes.push(crate::ProductMesh {
        name: "Original unsupported-model fixture".into(),
        mesh: Arc::new(Mesh::default()),
        color: [255; 3],
        opacity: 1.0,
        surface: None,
        transmission: Some(
            crate::transmission::Transmission::new(solidcore_record::Vec3::ZERO, None)
                .unwrap()
                .with_model(crate::glass_model::GlassModel { roughness: 0.3, ..Default::default() })
                .unwrap(),
        ),
        faces: Vec::new(),
    });
    let error = host.admit(&scene, &request(), JobOptions::default()).unwrap_err();
    assert!(error.message.contains("advanced Glass models"));
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn a_completed_task_is_dropped_while_its_full_reservation_still_pays_for_it() {
    let estimated = 1 << 20;
    let (host, admitted_at_drop) = probe_host(estimated);
    let job = host.submit(id(1, 1), 1, empty(), request()).unwrap();
    drain(&host);
    assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
    // While the task (and its scratch) was being dropped, its whole charge was still held.
    assert_eq!(admitted_at_drop.load(Ordering::Acquire), estimated + fixed_request_metadata_bytes());
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
    assert_eq!(admitted_at_drop.load(Ordering::Acquire), estimated + fixed_request_metadata_bytes(), "the task went before its charge");
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(job);
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
                caustic_work: None,
                cpu_workers: None,
                host_memory: None,
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
    let limit = 1024 * 1024 * 1024;
    let memory = super::super::host_memory::NativeMemoryPool::testing(limit);
    let host = ImageHost(Arc::new(Owner {
        executor: Some(Arc::new(Envelope(limit))),
        shared: ImageHost::shared_with_memory(limit, Some(memory)),
        kind: HostKind::NativeWorker,
        running: Mutex::new(None),
        _worker: None,
    }));
    assert_eq!(host.pool_limit_bytes(), limit);
    assert!(host.admit(&empty(), &request(), JobOptions::default()).is_ok());
    assert_eq!(host.admitted_bytes(), 0, "preflight does not allocate or reserve");
    let lease = host.0.shared.budget.reserve(600 * 1024 * 1024).unwrap();
    assert!(host.0.shared.budget.reserve(600 * 1024 * 1024).is_err());
    drop(lease);
    assert_eq!(host.admitted_bytes(), 0);
}
/// A registered executor exercises the shared publication boundary without a device.
/// Each pass encodes a real, distinct PNG; no final is returned with a preview.
struct CheckpointExecutor;
struct CheckpointWork {
    request: ImageRequest,
    cancel: CancelToken,
    samples: u32,
    target: u32,
    preview: Option<super::super::ImageWorkPreview>,
}
impl ImageWork for CheckpointWork {
    fn take_preview(&mut self) -> Option<super::super::ImageWorkPreview> {
        self.preview.take()
    }
    fn step(&mut self, _: usize) -> Result<TaskPoll> {
        self.cancel.check()?;
        self.samples += 1;
        let rgba = vec![self.samples as u8; (self.request.width * self.request.height * 4) as usize];
        let mut task = super::super::PngTask::new(&self.request, rgba, self.cancel.clone())?;
        let TaskPoll::Complete(png) = task.step(4)? else {
            return Err(JobError::new(ErrorCode::Internal, "small checkpoint fixture did not encode"));
        };
        if self.samples == self.target {
            Ok(TaskPoll::Complete(png))
        } else {
            self.preview = Some(super::super::ImageWorkPreview { png, samples_per_pixel: self.samples });
            Ok(TaskPoll::Pending(Progress { phase: Phase::Running, done: u64::from(self.samples), total: u64::from(self.target) }))
        }
    }
}
impl ImageExecutor for CheckpointExecutor {
    fn preview_samples(&self, options: JobOptions) -> Option<u32> {
        Some(options.samples)
    }
    fn descriptor(&self) -> Result<ExecutorDescriptor> {
        CpuExecutor.descriptor()
    }
    fn admit(&self, scene: &ProductScene, request: &ImageRequest, _: JobOptions) -> Result<Admission> {
        admit(scene, request)
    }
    fn begin(&self, _: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
        Ok(Box::new(CheckpointWork { request, cancel, samples: 0, target: options.samples, preview: None }))
    }
}

/// A valid registered producer need not use the built-in dimension-based PNG bound.
struct BoundedCheckpointExecutor {
    encoded_limit: usize,
}
struct BoundedCheckpointWork {
    inner: CheckpointWork,
    encoded_limit: usize,
}
impl BoundedCheckpointWork {
    fn retain_png(&self, png: Vec<u8>) -> Vec<u8> {
        assert!(png.len() <= self.encoded_limit);
        let mut retained = Vec::with_capacity(self.encoded_limit);
        retained.extend_from_slice(&png);
        assert_eq!(retained.capacity(), self.encoded_limit);
        retained
    }
}
impl ImageWork for BoundedCheckpointWork {
    fn take_preview(&mut self) -> Option<super::super::ImageWorkPreview> {
        self.inner.take_preview()
    }
    fn step(&mut self, units: usize) -> Result<TaskPoll> {
        let poll = self.inner.step(units)?;
        if let Some(mut preview) = self.inner.preview.take() {
            preview.png = self.retain_png(preview.png);
            self.inner.preview = Some(preview);
        }
        Ok(match poll {
            TaskPoll::Complete(png) => TaskPoll::Complete(self.retain_png(png)),
            pending => pending,
        })
    }
}
impl ImageExecutor for BoundedCheckpointExecutor {
    fn preview_samples(&self, options: JobOptions) -> Option<u32> {
        Some(options.samples)
    }
    fn descriptor(&self) -> Result<ExecutorDescriptor> {
        CpuExecutor.descriptor()
    }
    fn admit(&self, scene: &ProductScene, request: &ImageRequest, _: JobOptions) -> Result<Admission> {
        // Pay the complete built-in encoder envelope and the overlapping repack
        // allocation independently of the custom retained-output bound.
        let mut admission = admit(scene, request)?;
        let repack_bytes = self.encoded_limit as u64 + 4096;
        admission.preparation_bytes += repack_bytes;
        admission.estimated_total_bytes += repack_bytes;
        admission.encoded_limit = self.encoded_limit as u64;
        Ok(admission)
    }
    fn begin(&self, _: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
        Ok(Box::new(BoundedCheckpointWork {
            inner: CheckpointWork { request, cancel, samples: 0, target: options.samples, preview: None },
            encoded_limit: self.encoded_limit,
        }))
    }
}

fn registered_checkpoint_encoded_bound(encoded_limit: usize) {
    for cancel in [false, true] {
        let host = ImageHost::cooperative_with(Arc::new(BoundedCheckpointExecutor { encoded_limit }));
        let request = ImageRequest { width: 2, height: 2, ..request() };
        let options = JobOptions { samples: 5, ..Default::default() };
        let work_bytes = admit(&empty(), &request).unwrap().estimated_total_bytes + encoded_limit as u64 + 4096;
        let slot_bytes = encoded_limit as u64 + FRAME_METADATA_BYTES;
        let admission = host.admit(&empty(), &request, options).unwrap();
        assert_eq!(admission.encoded_limit, encoded_limit as u64);
        assert_eq!(admission.estimated_total_bytes, work_bytes + 2 * slot_bytes + FRAME_METADATA_BYTES);
        let job = host.submit_with_options(id(9023, 1), 57, empty(), request, options).unwrap();
        let admitted = host.admitted_bytes();
        let mut held = Vec::new();
        for samples in 1..=2 {
            for _ in 0..8 {
                host.pump(1).unwrap();
                let status = job.status().unwrap();
                assert_ne!(status.progress.phase, Phase::Failed, "custom encoded limit {encoded_limit}: {:?}", status.error);
                if job.preview().unwrap().is_some_and(|preview| preview.info.samples_per_pixel == samples) {
                    break;
                }
            }
            let preview = job.preview().unwrap().expect("an admitted custom PNG bound must publish its complete pass");
            assert_eq!(preview.info.samples_per_pixel, samples);
            assert_eq!(preview.png.capacity(), encoded_limit);
            assert_eq!(preview._reservation.bytes, slot_bytes);
            assert_eq!(preview.info.frame.png_sha256, format!("{:x}", Sha256::digest(preview.png())));
            let decoded = image::load_from_memory(preview.png()).unwrap().to_rgba8();
            assert_eq!(decoded.dimensions(), (2, 2));
            assert_eq!(decoded.as_raw(), &vec![samples as u8; 16]);
            assert!(job.frame().unwrap().is_none());
            assert_eq!(host.admitted_bytes(), admitted, "publication only transfers admitted credit");
            let running = host.0.running.lock().unwrap();
            let running = running.as_ref().unwrap();
            assert_eq!(running.reservation.as_ref().unwrap().bytes, work_bytes, "preview credits must not consume executor work");
            assert_eq!(running.checkpoints.as_ref().unwrap().slot_bytes, slot_bytes);
            held.push(preview);
        }
        if cancel {
            job.cancel().unwrap();
        }
        drain(&host);
        let status = job.status().unwrap();
        assert_eq!(status.progress.phase, if cancel { Phase::Cancelled } else { Phase::Completed });
        assert_eq!(status.preview_retention_skips, if cancel { 0 } else { 2 });
        assert!(job.preview().unwrap().is_none());
        assert!(job.work_released());
        let frame = job.frame().unwrap();
        if cancel {
            assert!(frame.is_none());
        } else {
            let frame = frame.as_ref().unwrap();
            assert_eq!(frame.png.capacity(), encoded_limit);
            assert_eq!(frame._reservation.bytes, slot_bytes);
            assert_eq!(frame.info.png_sha256, format!("{:x}", Sha256::digest(frame.png())));
            assert_eq!(image::load_from_memory(frame.png()).unwrap().to_rgba8().as_raw(), &vec![5; 16]);
        }
        for (index, preview) in held.iter().enumerate() {
            assert_eq!(preview.info.samples_per_pixel as usize, index + 1);
            assert_eq!(preview.info.frame.png_sha256, format!("{:x}", Sha256::digest(preview.png())));
            assert_eq!(image::load_from_memory(preview.png()).unwrap().to_rgba8().as_raw(), &vec![(index + 1) as u8; 16]);
        }
        drop(frame);
        drop(job);
        assert_eq!(host.admitted_bytes(), 2 * slot_bytes, "only externally pinned checkpoints retain admission");
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[test]
fn registered_executor_checkpoint_slots_follow_smaller_admitted_encoded_limit() {
    registered_checkpoint_encoded_bound(4096);
}

#[test]
fn registered_executor_checkpoint_slots_follow_larger_admitted_encoded_limit() {
    registered_checkpoint_encoded_bound(131_072);
}

#[test]
fn registered_executor_checkpoints_are_bounded_immutable_and_never_final() {
    let host = ImageHost::cooperative_with(Arc::new(CheckpointExecutor));
    let request = ImageRequest { width: 2, height: 2, ..request() };
    let options = JobOptions { samples: 5, ..Default::default() };
    let base = admit(&empty(), &request).unwrap();
    let expected = base.estimated_total_bytes + 2 * (base.encoded_limit + FRAME_METADATA_BYTES) + FRAME_METADATA_BYTES;
    assert_eq!(host.admit(&empty(), &request, options).unwrap().estimated_total_bytes, expected);
    let job = host.submit_with_options(id(9021, 1), 55, empty(), request, options).unwrap();
    let mut held = Vec::new();
    for samples in 1..3 {
        for _ in 0..8 {
            host.pump(1).unwrap();
            if job.preview().unwrap().is_some_and(|preview| preview.info.samples_per_pixel == samples) {
                break;
            }
        }
        let preview = job.preview().unwrap().unwrap();
        assert_eq!(preview.info.samples_per_pixel, samples);
        assert_eq!(preview.info.frame.job, job.id());
        assert_eq!(preview.info.frame.scene_generation, 55);
        assert_eq!(preview.info.frame.png_sha256, format!("{:x}", Sha256::digest(preview.png())));
        assert!(job.frame().unwrap().is_none());
        held.push(preview);
    }
    while host.pump(1).unwrap() {}
    assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
    assert_eq!(job.status().unwrap().preview_retention_skips, 2);
    let frame = job.frame().unwrap().unwrap();
    assert_eq!(frame.info.job, job.id());
    for snapshot in &held {
        assert_ne!(snapshot.png(), frame.png());
        assert_eq!(snapshot.info.frame.png_sha256, format!("{:x}", Sha256::digest(snapshot.png())));
    }
    drop(frame);
    drop(job);
    assert!(host.admitted_bytes() > 0);
    drop(held);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn cancelling_registered_checkpoint_work_preserves_held_snapshot_without_final() {
    let host = ImageHost::cooperative_with(Arc::new(CheckpointExecutor));
    let job = host
        .submit_with_options(
            id(9022, 1),
            56,
            empty(),
            ImageRequest { width: 2, height: 2, ..request() },
            JobOptions { samples: 5, ..Default::default() },
        )
        .unwrap();
    while job.preview().unwrap().is_none() {
        host.pump(1).unwrap();
    }
    let held = job.preview().unwrap().unwrap();
    job.cancel().unwrap();
    while host.pump(1).unwrap() {}
    assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
    assert!(job.preview().unwrap().is_none());
    assert!(job.frame().unwrap().is_none());
    assert_eq!(held.info.samples_per_pixel, 1);
    drop(job);
    assert_eq!(host.admitted_bytes(), held._reservation.bytes);
    drop(held);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn cancellation_phase_precedes_the_actual_work_release_barrier() {
    let (host, job, mut running, _) = pending_digest_fixture();
    assert!(!job.work_released());
    assert!(host.admitted_bytes() > 0);
    job.cancel().unwrap();
    assert!(!running.guarded_step(1));
    assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
    assert!(!job.work_released(), "terminal status cannot admit a competing near-pool-sized job yet");
    assert!(host.admitted_bytes() > 0, "the Running owner still holds its work reservation");
    drop(running);
    assert!(job.work_released());
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(job);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn queued_cancellation_releases_only_when_its_capture_and_reservation_drop() {
    let host = CpuHost::cooperative();
    let job = host.submit(id(9030, 1), 60, empty(), request()).unwrap();
    assert_eq!(job.cancel().unwrap().progress.phase, Phase::Cancelled);
    assert!(!job.work_released());
    assert!(host.admitted_bytes() > 0);
    assert!(!host.pump(1).unwrap());
    assert!(job.work_released());
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(job);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn completed_work_barrier_does_not_claim_external_final_images_are_free() {
    let (host, job, mut running, _) = pending_digest_fixture();
    while running.advance(4096).unwrap() {}
    assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
    assert!(!job.work_released());
    let frame = job.frame().unwrap().unwrap();
    drop(running);
    assert!(job.work_released());
    assert!(host.admitted_bytes() > 0, "completed images have their own retained leases");
    drop(job);
    assert!(host.admitted_bytes() > 0);
    drop(frame);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn host_shutdown_releases_both_queued_and_active_work_before_signalling() {
    let (host, active, running, _) = pending_digest_fixture();
    *host.0.running.lock().unwrap() = Some(running);
    let queued = host.submit(id(9031, 1), 61, empty(), request()).unwrap();
    assert!(!active.work_released());
    assert!(!queued.work_released());
    let budget = host.0.shared.budget.clone();
    drop(host);
    assert!(active.work_released());
    assert!(queued.work_released());
    assert_eq!(budget.used.load(Ordering::Acquire), 2 * fixed_request_metadata_bytes());
    drop(active);
    drop(queued);
    assert_eq!(budget.used.load(Ordering::Acquire), 0);
}

struct FailingBeginExecutor;
impl ImageExecutor for FailingBeginExecutor {
    fn descriptor(&self) -> Result<ExecutorDescriptor> {
        CpuExecutor.descriptor()
    }
    fn admit(&self, scene: &ProductScene, request: &ImageRequest, _: JobOptions) -> Result<Admission> {
        admit(scene, request)
    }
    fn begin(&self, _scene: ProductScene, _: ImageRequest, _: JobOptions, _: CancelToken) -> Result<Box<dyn ImageWork>> {
        Err(JobError::new(ErrorCode::InvalidScene, "original failing executor fixture"))
    }
}

#[test]
fn failed_begin_releases_captured_scene_and_reservation_before_signalling() {
    let host = ImageHost::cooperative_with(Arc::new(FailingBeginExecutor));
    let mesh = Arc::new(Mesh {
        positions: vec![solidcore_record::Vec3::ZERO, solidcore_record::Vec3::X, solidcore_record::Vec3::Y],
        normals: vec![solidcore_record::Vec3::Z; 3],
        triangles: vec![[0, 1, 2]],
        ..Default::default()
    });
    let captured = Arc::downgrade(&mesh);
    let scene = ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![crate::ProductMesh {
            name: "original capture".into(),
            mesh,
            color: [128; 3],
            opacity: 1.0,
            surface: None,
            transmission: None,
            faces: Vec::new(),
        }],
    };
    let job = host.submit(id(9032, 1), 62, scene, request()).unwrap();
    let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
    assert!(!job.work_released());
    assert!(captured.upgrade().is_some());
    assert!(Running::begin(work).is_none());
    assert!(captured.upgrade().is_none(), "the actual captured mesh must be released before the barrier");
    let status = job.status().unwrap();
    assert_eq!(status.progress.phase, Phase::Failed);
    assert_eq!(status.error.unwrap().message, "original failing executor fixture", "the fixture must reach executor begin");
    assert!(job.work_released());
    assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
    drop(job);
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
mod cpu_output_masters {
    use super::*;
    use crate::job::{CpuProgressiveOptions, OutputOptions, RaytraceOptions};
    fn fixture(host: &CpuHost, serial: u64) -> (JobHandle, Running) {
        let settings = OutputOptions {
            linear_exr: true,
            tone_map: crate::job::ToneMap::AcesFitted,
            finishing: crate::job::finishing::Finishing {
                bloom: Some(crate::job::bloom::Bloom { strength: 0.2, threshold: 0.0, radius: 1 }),
                ..Default::default()
            },
            ..Default::default()
        };
        let request = ImageRequest {
            integrator: Integrator::Raytrace,
            raytrace: Some(RaytraceOptions {
                progressive: Some(CpuProgressiveOptions { samples_per_pixel: 3, seed: 9 }),
                output: Some(Box::new(settings)),
                lights: Some(Vec::new()),
                ..Default::default()
            }),
            width: 2,
            height: 2,
            ..request()
        };
        let job = host.submit(id(9040, serial), 63, empty(), request).unwrap();
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        let running = Running::begin(work).unwrap();
        (job, running)
    }
    fn host() -> CpuHost {
        ImageHost(Arc::new(Owner {
            executor: None,
            shared: CpuHost::shared(MEMORY_LIMIT),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }))
    }
    fn hold_checkpoints(job: &JobHandle, running: &mut Running) -> Vec<Arc<RenderPreview>> {
        let mut held = Vec::new();
        for _ in 0..100_000 {
            if let Some(preview) = job.preview().unwrap()
                && held.last().is_none_or(|last: &Arc<RenderPreview>| last.info.frame_sequence != preview.info.frame_sequence)
            {
                held.push(preview);
            }
            if running.pending_png.as_ref().is_some_and(|png| png.exr.is_some()) {
                return held;
            }
            assert!(running.advance(1).unwrap());
        }
        panic!("small CPU master did not reach its final digest");
    }
    #[test]
    fn cpu_master_and_every_retained_preview_keep_exact_independent_leases() {
        let host = host();
        let (job, mut running) = fixture(&host, 1);
        let held = hold_checkpoints(&job, &mut running);
        assert_eq!(held.len(), 2);
        assert!(job.frame().unwrap().is_none());
        while running.advance(1).unwrap() {}
        let frame = job.frame().unwrap().unwrap();
        let raw = frame.linear_exr().unwrap();
        assert_eq!(frame.info.executor.as_str(), "cpu-raytrace");
        assert_eq!(frame.linear_master_info().unwrap().sha256, format!("{:x}", Sha256::digest(raw)));
        assert_eq!(frame._reservation.bytes, frame.png.capacity() as u64 + frame.exr.as_ref().unwrap().0.capacity() as u64 + FRAME_METADATA_BYTES);
        let preview_bytes: u64 = held.iter().map(|preview| preview._reservation.bytes).sum();
        let frame_bytes = frame._reservation.bytes;
        drop(running);
        assert!(job.work_released());
        drop(job);
        assert_eq!(host.admitted_bytes(), frame_bytes + preview_bytes);
        drop(frame);
        assert_eq!(host.admitted_bytes(), preview_bytes);
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }
    #[test]
    fn cancelling_cpu_master_digest_preserves_previous_final_and_pinned_preview_ownership() {
        let host = host();
        let (previous, mut previous_running) = fixture(&host, 1);
        while previous_running.advance(1).unwrap() {}
        drop(previous_running);
        let final_frame = previous.frame().unwrap().unwrap();
        let previous_png = final_frame.png().to_vec();
        let previous_raw = final_frame.linear_exr().unwrap().to_vec();
        let (job, mut running) = fixture(&host, 2);
        let held = hold_checkpoints(&job, &mut running);
        assert!(job.frame().unwrap().is_none());
        job.cancel().unwrap();
        assert_eq!(running.advance(1).unwrap_err().code, ErrorCode::Cancelled);
        drop(running);
        assert!(job.work_released());
        assert!(job.frame().unwrap().is_none());
        assert_eq!(final_frame.png(), previous_png);
        assert_eq!(final_frame.linear_exr().unwrap(), previous_raw);
        drop(job);
        drop(previous);
        let preview_bytes: u64 = held.iter().map(|preview| preview._reservation.bytes).sum();
        assert_eq!(host.admitted_bytes(), final_frame._reservation.bytes + preview_bytes);
        drop(final_frame);
        assert_eq!(host.admitted_bytes(), preview_bytes);
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[test]
fn registered_punctual_diagnostics_survive_work_release_and_retained_status_handles() {
    struct DiagnosticExecutor;
    struct DiagnosticWork(ImageTask);
    let counts = super::super::PunctualDiagnostics { near_source: 1_u64 << 33, radiance_limit: 7, shadow_offset_crossed: 11 };
    impl ImageWork for DiagnosticWork {
        fn punctual_diagnostics(&self) -> Option<super::super::PunctualDiagnostics> {
            Some(super::super::PunctualDiagnostics { near_source: 1_u64 << 33, radiance_limit: 7, shadow_offset_crossed: 11 })
        }
        fn step(&mut self, units: usize) -> Result<TaskPoll> {
            self.0.step(units)
        }
    }
    impl ImageExecutor for DiagnosticExecutor {
        fn descriptor(&self) -> Result<ExecutorDescriptor> {
            CpuExecutor.descriptor()
        }
        fn admit(&self, scene: &ProductScene, request: &ImageRequest, _: JobOptions) -> Result<Admission> {
            admit(scene, request)
        }
        fn begin(&self, scene: ProductScene, request: ImageRequest, _: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
            Ok(Box::new(DiagnosticWork(ImageTask::new(scene, request, cancel)?)))
        }
    }
    let host = CpuHost::cooperative_with(Arc::new(DiagnosticExecutor));
    let job = host.submit(id(9027, 1), 71, empty(), request()).unwrap();
    let retained = job.clone();
    drain(&host);
    assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
    assert_eq!(job.status().unwrap().punctual_diagnostics, Some(counts));
    assert!(job.work_released());
    assert!(job.frame().unwrap().is_some());
    drop(job);
    assert_eq!(retained.status().unwrap().punctual_diagnostics, Some(counts));
    drop(retained);
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
mod cpu_adaptive_checkpoints {
    use super::*;
    use crate::job::{
        CpuProgressiveOptions, OutputOptions, RaytraceOptions,
        adaptive::{AdaptiveSampling, PreviewStage},
    };
    #[test]
    fn cpu_adaptive_bounded_snapshots_span_pilot_reset_and_production_without_lending_the_final_master() {
        let host = ImageHost(Arc::new(Owner {
            executor: None,
            shared: CpuHost::shared(MEMORY_LIMIT),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }));
        let request = ImageRequest {
            width: 1,
            height: 1,
            integrator: Integrator::Raytrace,
            raytrace: Some(RaytraceOptions {
                progressive: Some(CpuProgressiveOptions { samples_per_pixel: 48, seed: 81 }),
                adaptive: Some(Box::new(AdaptiveSampling { pilot_samples: 16, min_samples: 16, ..Default::default() })),
                output: Some(Box::new(OutputOptions { linear_exr: true, ..Default::default() })),
                lights: Some(Vec::new()),
                ..Default::default()
            }),
            ..request()
        };
        let request_bytes = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        assert_eq!(
            request_bytes,
            (std::mem::size_of::<AdaptiveSampling>() + std::mem::size_of::<OutputOptions>()) as u64 + fixed_request_metadata_bytes()
        );
        let expected = admit(&empty(), &request).unwrap().estimated_total_bytes + request_bytes;
        let job = host.submit(id(9120, 1), 91, empty(), request).unwrap();
        assert_eq!(host.admitted_bytes(), expected);
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        let mut running = Running::begin(work).unwrap();
        assert_eq!(running.checkpoint_target, 64);
        let credits = running.checkpoints.as_ref().unwrap();
        assert_eq!(credits.capacity, 2 * credits.slot_bytes + FRAME_METADATA_BYTES);
        let mut held: Vec<Arc<RenderPreview>> = Vec::new();
        let mut saw_reset_production = false;
        let mut published = Vec::new();
        for _ in 0..200_000 {
            if let Some(png) = running.pending_png.as_ref()
                && let Some(absolute) = png.preview_samples
            {
                assert!(job.frame().unwrap().is_none());
                if absolute == 17 {
                    drop(held.remove(0));
                }
                let before = job.status().unwrap();
                assert!(running.advance(1).unwrap());
                let status = job.status().unwrap();
                let preview = job.preview().unwrap().expect("publication or retention keeps a complete pass available");
                if status.preview_retention_skips != before.preview_retention_skips {
                    assert!((3..=16).contains(&absolute), "only the two pinned pilot snapshots skip publication");
                    assert_eq!(status.preview_retention_skips, before.preview_retention_skips + 1);
                    let retained = held.last().unwrap();
                    assert_eq!(retained.info.samples_per_pixel, 2);
                    assert!(Arc::ptr_eq(&preview, retained), "a skipped pass keeps the pinned second publication");
                    assert_eq!(serde_json::to_value(&preview.info).unwrap(), serde_json::to_value(&retained.info).unwrap());
                    assert_eq!(serde_json::to_value(&status.preview).unwrap(), serde_json::to_value(&before.preview).unwrap());
                    assert_eq!(serde_json::to_value(status.preview.as_ref().unwrap()).unwrap(), serde_json::to_value(&retained.info).unwrap());
                    assert_eq!(preview.info.frame.png_sha256, format!("{:x}", Sha256::digest(preview.png())));
                } else {
                    let metadata = preview.info.adaptive.unwrap();
                    assert_eq!(preview.info.samples_per_pixel, absolute);
                    assert_eq!(metadata.pilot_samples, 16);
                    assert_eq!((metadata.minimum_production_samples, metadata.maximum_production_samples), (16, 48));
                    assert_eq!(preview.info.frame.png_sha256, format!("{:x}", Sha256::digest(preview.png())));
                    if absolute <= 16 {
                        assert_eq!((metadata.stage, metadata.completed, metadata.target), (PreviewStage::Pilot, absolute, 16));
                    } else {
                        assert_eq!((metadata.stage, metadata.completed, metadata.target), (PreviewStage::Production, absolute - 16, 48));
                        saw_reset_production = true;
                    }
                    published.push(absolute);
                    if absolute <= 2 {
                        held.push(preview);
                    }
                }
                assert_eq!(host.admitted_bytes(), expected);
                continue;
            }
            if !running.advance(1).unwrap() {
                break;
            }
        }
        assert!(saw_reset_production);
        assert_eq!(&published[..2], &[1, 2]);
        assert!(published.contains(&17) && published.contains(&63));
        let status = job.status().unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        assert_eq!(status.preview_retention_skips, 14);
        assert!(job.preview().unwrap().is_none());
        let report = status.sampling.unwrap();
        assert_eq!((report.pilot_samples_per_pixel, report.minimum_production_samples, report.maximum_production_samples), (16, 16, 16));
        assert_eq!((report.production_samples, report.primary_path_evaluations, report.pixels_at_cap), (16, 32, 0));
        drop(running);
        assert!(job.work_released());
        let frame = job.frame().unwrap().unwrap();
        assert!(frame.linear_exr().is_some());
        let retained = held.iter().map(|p| p._reservation.bytes).sum::<u64>();
        assert_eq!(
            host.admitted_bytes(),
            retained + frame._reservation.bytes + request_bytes,
            "the original captured adaptive/output boxes retain their independent request lease"
        );
        drop(job);
        drop(frame);
        assert_eq!(host.admitted_bytes(), retained);
        drop(held);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn celestial_cancelled_capture_retains_only_exact_boxes_and_fixed_header_until_last_handle_drop() {
    use super::super::{CpuPathOptions, CpuProgressiveOptions, CpuSky, RaytraceOptions, Sun};
    let host = ImageHost(Arc::new(Owner {
        executor: None,
        shared: CpuHost::shared(MEMORY_LIMIT),
        kind: HostKind::NativeWorker,
        running: Mutex::new(None),
        _worker: None,
    }));
    let options = RaytraceOptions {
        path: Some(CpuPathOptions { max_depth: 2 }),
        progressive: Some(CpuProgressiveOptions { samples_per_pixel: 3, seed: 81 }),
        sun: Some(Box::new(Sun { direction: [0.0, 0.0, 1.0], angular_radius_rad: 0.02, radiance: [3.0, 2.0, 1.0] })),
        sky: Some(Box::new(CpuSky::default())),
        ..Default::default()
    };
    let fixed = ImageRequest::fixed_cpu_metadata_bytes();
    assert_eq!(
        fixed,
        (std::mem::size_of::<Option<super::super::CausticOptions>>()
            + std::mem::size_of::<Option<Box<Sun>>>()
            + std::mem::size_of::<Option<Box<CpuSky>>>()
            + std::mem::size_of::<Option<super::super::volume::HomogeneousVolume>>()) as u64
    );
    let captured = ImageRequest { integrator: Integrator::Raytrace, raytrace: Some(options.clone()), ..request() };
    assert_eq!(captured.retained_resource_metadata_bytes(), fixed);
    let retained = options.retained_bytes().unwrap() + fixed;
    let job = host.submit(id(9170, 1), 101, empty(), captured).unwrap();
    let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
    job.cancel().unwrap();
    assert!(!job.work_released());
    drop(work);
    assert!(job.work_released());
    assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
    assert!(job.frame().unwrap().is_none());
    assert_eq!(host.admitted_bytes(), retained, "fixed request headers cannot borrow a released Work or absent final-frame lease");
    assert_eq!(job.0._request_reservation.as_ref().unwrap().bytes, retained);
    let held = job.clone();
    drop(job);
    assert_eq!(host.admitted_bytes(), retained);
    drop(held);
    assert_eq!(host.admitted_bytes(), 0);
    // Omitted boxes introduce no source buffer, but their actual inline header
    // growth remains explicitly billed even for the embedded None variant.
    let plain = request();
    assert!(plain.raytrace.is_none());
    assert_eq!(plain.retained_resource_metadata_bytes(), fixed);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
mod prepared_optical_submission {
    use super::*;
    use crate::job::{DielectricBoundary, DielectricOptions, RaytraceOptions};
    use solidcore_record::Vec3;

    fn host(limit: u64) -> CpuHost {
        ImageHost(Arc::new(Owner {
            executor: None,
            shared: CpuHost::shared(limit),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }))
    }
    fn capture() -> (ProductScene, ImageRequest) {
        let mesh = Arc::new(Mesh {
            positions: (0..8)
                .map(|i| Vec3::new(if i & 1 == 0 { -1.0 } else { 1.0 }, if i & 2 == 0 { -1.0 } else { 1.0 }, if i & 4 == 0 { 0.0 } else { 2.0 }))
                .collect(),
            triangles: vec![
                [0, 2, 3],
                [0, 3, 1],
                [4, 5, 7],
                [4, 7, 6],
                [0, 4, 6],
                [0, 6, 2],
                [1, 3, 7],
                [1, 7, 5],
                [0, 1, 5],
                [0, 5, 4],
                [2, 6, 7],
                [2, 7, 3],
            ],
            ..Default::default()
        });
        let scene = ProductScene {
            bounds: mesh.bounds(),
            meshes: vec![crate::ProductMesh {
                name: "captured box".into(),
                mesh,
                color: [255; 3],
                opacity: 1.0,
                surface: None,
                faces: Vec::new(),
                transmission: None,
            }],
        };
        let mut request = ImageRequest { integrator: Integrator::Raytrace, width: 1, height: 1, ..request() };
        request.camera.set_view(solidcraft_viewport::StandardView::Top);
        request.raytrace = Some(RaytraceOptions {
            lights: Some(Vec::new()),
            environment: Some(crate::job::RaytraceEnvironmentOptions { color: [1.0; 3], intensity: 1.0 }),
            dielectrics: vec![DielectricOptions {
                model: None,
                body_index: 0,
                body_name: "captured box".into(),
                boundary: DielectricBoundary::ClosedBox,
                ior: 1.5,
                absorption_per_mm: [0.0; 3],
                thickness_mm: 0.0,
                shadow_transmission: false,
                subsurface: None,
            }],
            ..Default::default()
        });
        (scene, request)
    }

    #[test]
    fn prepared_optics_debit_once_and_transfer_work_and_retained_leases_without_enqueuing_early() {
        let host = host(MEMORY_LIMIT);
        let (scene, request) = capture();
        let source = Arc::downgrade(&scene.meshes[0].mesh);
        let retained = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        let prepared = host.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default()).unwrap();
        assert_eq!(prepared.proof.len(), 1);
        assert!(prepared.options.is_none());
        assert!(host.0.shared.queue.lock().unwrap().is_empty(), "preparation has no job identity or queued work");
        let expected = prepared.admission().estimated_total_bytes + retained;
        assert_eq!(host.admitted_bytes(), expected);
        assert_eq!(prepared.reservation.bytes, prepared.admission().estimated_total_bytes);
        assert_eq!(prepared.request_reservation.as_ref().unwrap().bytes, retained);
        let job = host.submit_prepared(id(801, 1), 99, prepared).unwrap();
        assert_eq!(host.admitted_bytes(), expected, "enqueue moves rather than reserves credit");
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        assert_eq!(work.proof.len(), 1);
        let mut running = Running::begin(work).unwrap();
        for _ in 0..100_000 {
            if !running.advance(1).unwrap() {
                break;
            }
        }
        assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
        assert_eq!(job.id(), id(801, 1));
        drop(running);
        assert!(source.upgrade().is_none(), "certified source drops with actual work");
        let frame = job.frame().unwrap().unwrap();
        assert_eq!(host.admitted_bytes(), retained + frame._reservation.bytes);
        job.0.data.lock().unwrap().frame = None;
        drop(frame);
        assert_eq!(host.admitted_bytes(), retained, "captured option names/capacities outlive frame eviction");
        let clone = job.clone();
        drop(job);
        assert_eq!(host.admitted_bytes(), retained);
        drop(clone);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[test]
    fn preparation_drop_cancel_wrong_host_and_exact_pool_boundary_release_all_captures() {
        let original = host(MEMORY_LIMIT);
        let (scene, request) = capture();
        let prepared = original.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default()).unwrap();
        let required = original.admitted_bytes();
        drop(prepared);
        assert_eq!(original.admitted_bytes(), 0);
        for limit in [required - 1, required] {
            let host = host(limit);
            let (scene, request) = capture();
            let source = Arc::downgrade(&scene.meshes[0].mesh);
            let result = host.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default());
            assert_eq!(result.is_ok(), limit == required);
            drop(result);
            assert_eq!(host.admitted_bytes(), 0);
            assert!(source.upgrade().is_none());
        }
        let cancel = CancelToken::default();
        let (scene, request) = capture();
        let prepared = original.prepare_submission(scene, request, JobOptions::default(), None, cancel.clone()).unwrap();
        cancel.cancel();
        assert_eq!(original.submit_prepared(id(802, 1), 0, prepared).err().unwrap().code, ErrorCode::Cancelled);
        assert_eq!(original.admitted_bytes(), 0);
        let (scene, request) = capture();
        let prepared = original.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default()).unwrap();
        let other = host(MEMORY_LIMIT);
        assert_eq!(other.submit_prepared(id(802, 2), 0, prepared).err().unwrap().code, ErrorCode::WrongOwner);
        assert_eq!((original.admitted_bytes(), other.admitted_bytes()), (0, 0));
        assert!(original.0.shared.queue.lock().unwrap().is_empty());
    }

    #[test]
    fn strict_host_and_preparation_keep_precise_geometry_refusal_without_pool_or_queue_leaks() {
        let host = host(MEMORY_LIMIT);
        let (mut scene, request) = capture();
        Arc::make_mut(&mut scene.meshes[0].mesh).positions[0].z += 0.125;
        for error in [
            host.admit(&scene, &request, JobOptions::default()).err().unwrap(),
            host.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default()).err().unwrap(),
        ] {
            assert_eq!(error.code, ErrorCode::InvalidScene);
            assert!(error.message.contains("box face is not axis aligned"), "{}", error.message);
        }
        assert_eq!(host.admitted_bytes(), 0);
        assert!(host.0.shared.queue.lock().unwrap().is_empty());
    }
    #[test]
    fn borrowed_enqueue_keeps_failed_capsule_credit_and_source_until_worker_retirement() {
        let host = host(MEMORY_LIMIT);
        let (scene, request) = capture();
        let source = Arc::downgrade(&scene.meshes[0].mesh);
        let cancel = CancelToken::default();
        let prepared = host.prepare_submission(scene, request, JobOptions::default(), None, cancel.clone()).unwrap();
        let mut slot = Some(prepared);
        let own_bytes = host.admitted_bytes();
        let first = host.submit(id(803, 1), 0, empty(), super::request()).unwrap();
        let second = host.submit(id(803, 2), 0, empty(), super::request()).unwrap();
        let queued_bytes = host.admitted_bytes() - own_bytes;
        let error = host.try_submit_prepared(id(804, 1), 0, &mut slot).err().unwrap();
        assert_eq!(error.code, ErrorCode::QueueFull);
        assert!(slot.is_some() && source.upgrade().is_some());
        assert_eq!(host.admitted_bytes(), own_bytes + queued_bytes);
        // A racing cancellation does not make UI completion destroy queued
        // captures. Background draining will reclaim their work later.
        first.cancel().unwrap();
        assert_eq!(host.try_submit_prepared(id(804, 1), 0, &mut slot).err().unwrap().code, ErrorCode::QueueFull);
        assert_eq!(host.0.shared.queue.lock().unwrap().len(), 2);
        cancel.cancel();
        assert_eq!(host.try_submit_prepared(id(804, 1), 0, &mut slot).err().unwrap().code, ErrorCode::Cancelled);
        assert!(slot.is_some());
        let other = self::host(MEMORY_LIMIT);
        assert_eq!(other.try_submit_prepared(id(804, 1), 0, &mut slot).err().unwrap().code, ErrorCode::WrongOwner);
        assert_eq!(host.admitted_bytes(), own_bytes + queued_bytes);
        let (retire, receiver) = std::sync::mpsc::sync_channel(1);
        let (ack, released) = std::sync::mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            drop(receiver.recv().unwrap());
            ack.send(()).unwrap();
        });
        retire.send(slot.take().unwrap()).unwrap();
        released.recv().unwrap();
        worker.join().unwrap();
        assert!(slot.is_none() && source.upgrade().is_none());
        assert_eq!(host.admitted_bytes(), queued_bytes, "retirement ack follows actual owner/lease disposal");
        second.cancel().unwrap();
        drop(host.0.shared.queue.lock().unwrap().drain(..).collect::<Vec<_>>());
        drop((first, second));
        assert_eq!(host.admitted_bytes(), 0);
    }
    #[test]
    fn absent_optics_preserves_admission_and_has_no_proof_or_options_backing() {
        let host = host(MEMORY_LIMIT);
        for integrator in [Integrator::Raster, Integrator::Raytrace] {
            let request = ImageRequest { integrator, ..super::request() };
            let expected = super::admit(&empty(), &request).unwrap();
            let fixed = request.retained_resource_metadata_bytes();
            let prepared = host.prepare_submission(empty(), request, JobOptions::default(), None, CancelToken::default()).unwrap();
            assert_eq!(serde_json::to_value(prepared.admission()).unwrap(), serde_json::to_value(expected).unwrap());
            assert_eq!((prepared.proof.len(), prepared.proof.capacity()), (0, 0));
            assert!(prepared.options.is_none());
            assert_eq!(
                prepared.request_reservation.as_ref().unwrap().bytes,
                fixed,
                "None source paths still retain the exact CPU9 fixed header independently of Work"
            );
            assert_eq!(host.admitted_bytes(), expected.estimated_total_bytes + fixed);
            drop(prepared);
            assert_eq!(host.admitted_bytes(), 0);
        }
    }

    fn general_capture() -> (ProductScene, ImageRequest) {
        let (mut scene, mut request) = capture();
        // A rotated closed mesh is outside the legacy axis-aligned box subset.
        for p in &mut Arc::make_mut(&mut scene.meshes[0].mesh).positions {
            *p = Vec3::new((p.x + p.z) * std::f64::consts::FRAC_1_SQRT_2, p.y, (p.z - p.x) * std::f64::consts::FRAC_1_SQRT_2);
        }
        scene.bounds = scene.meshes[0].mesh.bounds();
        let options = request.raytrace.as_mut().unwrap();
        options.path = Some(crate::job::CpuPathOptions { max_depth: 16 });
        options.progressive = Some(crate::job::CpuProgressiveOptions { samples_per_pixel: 4, seed: 9 });
        options.shadows = true;
        options.dielectrics[0].boundary = DielectricBoundary::ClosedMesh;
        (scene, request)
    }

    #[test]
    fn general_glass_strict_admission_and_preparation_refuse_the_same_boundary_before_enqueue() {
        let host = host(MEMORY_LIMIT);
        for inward in [false, true] {
            let (mut scene, request) = general_capture();
            let mesh = Arc::make_mut(&mut scene.meshes[0].mesh);
            let expected = if inward {
                for t in &mut mesh.triangles {
                    t.swap(1, 2);
                }
                "outward"
            } else {
                mesh.triangles.pop();
                "two oppositely"
            };
            let source = Arc::downgrade(&scene.meshes[0].mesh);
            let original_triangles = scene.meshes[0].mesh.triangles.clone();
            let original_request = serde_json::to_value(&request).unwrap();
            for error in [
                crate::raytrace::image::admit(&scene, &request, request.raytrace.as_ref().unwrap()).err().unwrap(),
                super::admit(&scene, &request).err().unwrap(),
                host.admit(&scene, &request, JobOptions::default()).err().unwrap(),
            ] {
                assert_eq!(error.code, ErrorCode::InvalidScene);
                assert!(error.message.contains(expected) || (!inward && error.message.contains("closed edges")), "{error}");
                assert_eq!(host.admitted_bytes(), 0);
            }
            assert_eq!(scene.meshes[0].mesh.triangles, original_triangles);
            assert_eq!(serde_json::to_value(&request).unwrap(), original_request);
            let error = host.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default()).err().unwrap();
            assert_eq!(error.code, ErrorCode::InvalidScene);
            assert!(error.message.contains(expected) || (!inward && error.message.contains("closed edges")), "{error}");
            assert!(source.upgrade().is_none());
            assert_eq!(host.admitted_bytes(), 0);
            assert!(host.0.shared.queue.lock().unwrap().is_empty(), "failed Check never creates a render job");
        }
    }

    #[test]
    fn general_glass_prepared_proof_moves_with_exact_source_and_releases_work_before_retained_metadata() {
        let host = host(MEMORY_LIMIT);
        let (scene, request) = general_capture();
        let source = Arc::downgrade(&scene.meshes[0].mesh);
        let exact_mesh = Arc::as_ptr(&scene.meshes[0].mesh);
        let original_request = serde_json::to_value(&request).unwrap();
        let retained = retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        let prepared = host.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default()).unwrap();
        assert_eq!((prepared.proof.len(), prepared.proof.capacity()), (1, 1));
        assert_eq!(Arc::as_ptr(&prepared.scene.meshes[0].mesh), exact_mesh);
        assert_eq!(serde_json::to_value(prepared.request()).unwrap(), original_request);
        assert!(host.0.shared.queue.lock().unwrap().is_empty());
        let required = prepared.admission().estimated_total_bytes + retained;
        assert_eq!(host.admitted_bytes(), required);
        let job = host.submit_prepared(id(805, 1), 7, prepared).unwrap();
        assert_eq!(host.admitted_bytes(), required, "publication only transfers existing credit");
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        assert_eq!(Arc::as_ptr(&work.scene.meshes[0].mesh), exact_mesh);
        assert_eq!(work.proof.len(), 1);
        let mut running = Running::begin(work).unwrap();
        for _ in 0..100_000 {
            if !running.advance(1).unwrap() {
                break;
            }
        }
        assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
        drop(running);
        assert!(source.upgrade().is_none());
        assert!(job.work_released());
        let frame = job.frame().unwrap().unwrap();
        assert_eq!(host.admitted_bytes(), retained + frame._reservation.bytes);
        job.0.data.lock().unwrap().frame = None;
        drop(frame);
        assert_eq!(host.admitted_bytes(), retained, "general proof and every certificate buffer belong to Work");
        drop(job);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[test]
    fn general_glass_proof_exact_pool_cancel_and_wrong_host_never_release_credit_before_ownership() {
        let original = host(MEMORY_LIMIT);
        let (scene, request) = general_capture();
        let prepared = original.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default()).unwrap();
        let required = original.admitted_bytes();
        drop(prepared);
        assert_eq!(original.admitted_bytes(), 0);
        for limit in [required - 1, required] {
            let host = host(limit);
            let (scene, request) = general_capture();
            let source = Arc::downgrade(&scene.meshes[0].mesh);
            let result = host.prepare_submission(scene, request, JobOptions::default(), None, CancelToken::default());
            assert_eq!(result.is_ok(), limit == required);
            drop(result);
            assert!(source.upgrade().is_none());
            assert_eq!(host.admitted_bytes(), 0);
        }
        let other = host(MEMORY_LIMIT);
        for cancel_after_preparation in [false, true] {
            let (scene, request) = general_capture();
            let source = Arc::downgrade(&scene.meshes[0].mesh);
            let cancel = CancelToken::default();
            let prepared = original.prepare_submission(scene, request, JobOptions::default(), None, cancel.clone()).unwrap();
            let mut slot = Some(prepared);
            if cancel_after_preparation {
                cancel.cancel();
            }
            let result = if cancel_after_preparation {
                original.try_submit_prepared(id(806, 1), 0, &mut slot)
            } else {
                other.try_submit_prepared(id(806, 2), 0, &mut slot)
            };
            assert_eq!(result.err().unwrap().code, if cancel_after_preparation { ErrorCode::Cancelled } else { ErrorCode::WrongOwner });
            assert!(slot.is_some() && source.upgrade().is_some());
            assert_eq!(original.admitted_bytes(), required, "failed borrowed enqueue retains the exact certificate owner");
            assert_eq!(other.admitted_bytes(), 0);
            drop(slot);
            assert!(source.upgrade().is_none());
            assert_eq!(original.admitted_bytes(), 0);
        }
    }
}

#[test]
fn independently_retained_metadata_never_borrows_work_credit_and_rejects_a_different_host_pool() {
    let host = CpuHost::cooperative();
    let first = host.prepare_submission(empty(), request(), JobOptions::default(), None, CancelToken::default()).unwrap();
    let work = host.admitted_bytes();
    let lease = first.reserve_retained_metadata(257).unwrap();
    assert_eq!(lease.bytes(), 257);
    assert_eq!(host.admitted_bytes(), work + 257);
    drop(first);
    assert_eq!(host.admitted_bytes(), 257, "independent backing survives actual Work capsule release");
    let second =
        host.prepare_submission_with_metadata(empty(), request(), JobOptions::default(), None, CancelToken::default(), Some(&lease)).unwrap();
    assert_eq!(host.admitted_bytes(), work + 257, "existing retained credit is counted but not reserved twice");
    let other = CpuHost::cooperative();
    assert_eq!(
        other
            .prepare_submission_with_metadata(empty(), request(), JobOptions::default(), None, CancelToken::default(), Some(&lease))
            .err()
            .unwrap()
            .code,
        ErrorCode::WrongOwner
    );
    assert_eq!(other.admitted_bytes(), 0);
    drop(second);
    assert_eq!(host.admitted_bytes(), 257);
    drop(lease);
    assert_eq!(host.admitted_bytes(), 0);
}

mod capture_credit {
    use super::*;

    struct MetadataExecutor(Arc<AtomicU64>);
    impl ImageExecutor for MetadataExecutor {
        fn descriptor(&self) -> Result<ExecutorDescriptor> {
            let mut descriptor = CpuExecutor.descriptor()?;
            // Deliberately noncanonical order: resolution sorts both lists.
            // The full raw descriptor still must remain exactly pinned.
            descriptor.profiles[0].features.push(crate::RenderFeature::Hdri);
            descriptor.profiles[0].formats.insert(0, RenderFormat::Exr);
            let mut final_image = descriptor.profiles[0].clone();
            final_image.mode = RenderMode::Final;
            descriptor.profiles.push(final_image);
            let state = self.0.load(Ordering::Acquire);
            descriptor.id = ExecutorId::new(if state == 1 { "changed-capture-map" } else { "capture-map" }).unwrap();
            if state == 3 {
                descriptor.scratch.sample_bytes_per_pixel += 1;
            }
            Ok(descriptor)
        }
        fn admit(&self, scene: &ProductScene, request: &ImageRequest, _options: JobOptions) -> Result<Admission> {
            let mut clean = request.clone();
            clean.textures = None;
            let mut result = admit(scene, &clean)?;
            if let Some(maps) = &request.textures {
                result.estimated_total_bytes += maps.working_bytes();
            }
            // Deliberately change layout metadata after the first pre-submit
            // check, without changing executor identity or profile.
            if self.0.load(Ordering::Acquire) == 2 {
                self.0.store(3, Ordering::Release);
            }
            Ok(result)
        }
        fn begin(&self, _scene: ProductScene, _request: ImageRequest, _options: JobOptions, _cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
            Err(JobError::new(ErrorCode::InvalidRequest, "controlled capture factory failure"))
        }
    }
    fn registered(limit: u64, changed: Arc<AtomicU64>) -> ImageHost {
        ImageHost(Arc::new(Owner {
            executor: Some(Arc::new(MetadataExecutor(changed))),
            shared: ImageHost::shared(limit),
            kind: HostKind::Cooperative,
            running: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            _worker: None,
        }))
    }
    #[test]
    fn capture_registered_credit_has_actual_host_cap_and_pins_descriptor_token_and_retained_context() {
        let changed = Arc::new(AtomicU64::new(0));
        let host = registered(MEMORY_LIMIT, changed.clone());
        let cancel = CancelToken::default();
        let bytes = super::super::super::image::MAX_JOB_BYTES + 1;
        let lease = host.reserve_capture(bytes, CaptureProfile::Registered, &cancel, None).unwrap();
        assert_eq!(host.admitted_bytes(), bytes, "registered capture must not inherit Compact's smaller CPU cap");
        drop(lease);
        assert_eq!(host.admitted_bytes(), 0);
        assert!(
            matches!(host.reserve_capture(1,CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact),&cancel,None),Err(e) if e.code==ErrorCode::WrongOwner)
        );
        let retained = RetainedMetadata(host.0.shared.budget.reserve(123).unwrap());
        let mut lease = host.reserve_capture(100, CaptureProfile::Registered, &cancel, Some(&retained)).unwrap();
        assert_eq!(host.admitted_bytes(), 223);
        lease.try_extend(70, &cancel).unwrap();
        assert_eq!(lease.bytes(), 170);
        assert_eq!(host.admitted_bytes(), 293);
        let foreign = registered(MEMORY_LIMIT, Arc::new(AtomicU64::new(0)));
        let foreign_retained = RetainedMetadata(foreign.0.shared.budget.reserve(123).unwrap());
        assert!(matches!(host.reserve_capture(1,CaptureProfile::Registered,&cancel,Some(&foreign_retained)),Err(e) if e.code==ErrorCode::WrongOwner));
        let other = CancelToken::default();
        assert_eq!(lease.try_extend(1, &other).unwrap_err().code, ErrorCode::WrongOwner);
        assert_eq!(host.admitted_bytes(), 293);
        changed.store(1, Ordering::Release);
        assert_eq!(lease.try_extend(1, &cancel).unwrap_err().code, ErrorCode::HostUnavailable);
        assert_eq!(host.admitted_bytes(), 293);
        changed.store(0, Ordering::Release);
        cancel.cancel();
        assert_eq!(lease.try_extend(1, &cancel).unwrap_err().code, ErrorCode::Cancelled);
        drop(lease);
        assert_eq!(host.admitted_bytes(), 123);
        drop(retained);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    fn cpu(limit: u64) -> ImageHost {
        ImageHost(Arc::new(Owner {
            executor: None,
            shared: ImageHost::shared(limit),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }))
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    fn mapped() -> (ProductScene, ImageRequest, Weak<Vec<u8>>) {
        use crate::{
            job::{
                CpuProgressiveOptions, DirectionalLightOptions, OutputOptions, RaytraceOptions,
                textures::{BaseColorMode, PlanarFrame, RuntimeTextures, TextureBinding, TextureMap},
            },
            texture_input::MapRole,
        };
        use image::ImageEncoder;
        use solidcraft_viewport::surface::OpaqueSurface;
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes).write_image(&[128, 64, 32, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        let map = TextureMap::new(bytes, MapRole::BaseColor).unwrap();
        let weak = map.source().encoded_weak();
        let binding = TextureBinding {
            body_index: 0,
            body_name: "Mapped receiver".into(),
            frame: PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] },
            base_color: Some(map),
            normal: None,
            roughness: None,
            metallic: None,
            base_color_mode: BaseColorMode::Replace,
            flip_v: false,
            normal_strength: 1.0,
        };
        let maps = Arc::new(RuntimeTextures::new(vec![binding]).unwrap());
        let mesh = Mesh {
            positions: vec![
                solidcore_record::Vec3::new(-10., -10., 0.),
                solidcore_record::Vec3::new(10., -10., 0.),
                solidcore_record::Vec3::new(10., 10., 0.),
                solidcore_record::Vec3::new(-10., 10., 0.),
            ],
            normals: vec![solidcore_record::Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            tri_face: vec![0, 0],
            ..Default::default()
        };
        let source = ProductScene {
            bounds: mesh.bounds(),
            meshes: vec![crate::ProductMesh {
                name: "Mapped receiver".into(),
                mesh: Arc::new(mesh),
                color: [255; 3],
                opacity: 1.0,
                surface: Some(OpaqueSurface::new(solidcore_record::Vec3::new(1., 1., 1.), 0., 0.5, 1.).unwrap()),
                transmission: None,
                faces: Vec::new(),
            }],
        };
        let mut image = request();
        image.width = 2;
        image.height = 2;
        image.camera.distance = 4.;
        image.camera.set_view(solidcraft_viewport::StandardView::Top);
        image.integrator = Integrator::Raytrace;
        image.textures = Some(maps);
        image.raytrace = Some(RaytraceOptions {
            progressive: Some(CpuProgressiveOptions { samples_per_pixel: 2, seed: 17 }),
            lights: Some(vec![DirectionalLightOptions { direction: [0., 0., 1.], color: [1.; 3], intensity: std::f64::consts::PI }]),
            shadows: false,
            output: Some(Box::new(OutputOptions { linear_exr: true, ..Default::default() })),
            ..Default::default()
        });
        (source, image, weak)
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn capture_cpu_exact_cap_transfer_keeps_real_map_png_raw_and_releases_only_work_backing() {
        let render = |capture_excess: Option<u64>| {
            let (scene, image, weak) = mapped();
            let probe = cpu(MEMORY_LIMIT);
            let admission = probe.admit(&scene, &image, JobOptions::default()).unwrap();
            let retained = retained_request_bytes(&image).unwrap() + image.retained_resource_metadata_bytes();
            let total = admission.estimated_total_bytes + retained;
            drop(probe);
            let extra = capture_excess.unwrap_or(0);
            let host = cpu(total + extra + 123);
            let plan = RetainedMetadata(host.0.shared.budget.reserve(123).unwrap());
            let cancel = CancelToken::default();
            let prepared = if capture_excess.is_some() {
                let bytes = if extra == 0 { image.textures.as_ref().unwrap().captured_bytes().unwrap() } else { total + extra };
                let mut credit = host
                    .reserve_capture(bytes / 2, CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact), &cancel, Some(&plan))
                    .unwrap();
                credit.try_extend(bytes - bytes / 2, &cancel).unwrap();
                assert_eq!(host.admitted_bytes(), bytes + 123);
                host.prepare_submission_with_capture(scene, image, JobOptions::default(), None, cancel, Some(&plan), credit).unwrap()
            } else {
                host.prepare_submission_with_metadata(scene, image, JobOptions::default(), None, cancel, Some(&plan)).unwrap()
            };
            assert_eq!(prepared.admission().estimated_total_bytes, admission.estimated_total_bytes + extra);
            assert_eq!(prepared.admission().preparation_bytes, admission.preparation_bytes + extra);
            assert_eq!(host.admitted_bytes(), total + extra + 123, "exact pool leaves no room for a second combined reserve");
            let job = host.submit_prepared(id(9510, if capture_excess.is_some() { 2 } else { 1 }), 1, prepared).unwrap();
            assert!(job.0.request.textures.is_none(), "Record owns summary only, never captured map backing");
            let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
            let mut running = Running::begin(work).unwrap();
            assert!(weak.upgrade().is_some(), "the actual CPU factory received the full captured map source");
            for _ in 0..200000 {
                if !running.guarded_step(16384) {
                    break;
                }
            }
            assert_eq!(job.status().unwrap().progress.phase, Phase::Completed);
            let frame = job.frame().unwrap().unwrap();
            let png = frame.png().to_vec();
            let raw = frame.linear_exr().unwrap().to_vec();
            let pixels = image::load_from_memory(&png).unwrap().to_rgba8();
            assert_eq!(pixels.dimensions(), (2, 2));
            assert!(
                pixels.pixels().all(|p| p[0] > p[1] && p[1] > p[2] && p[2] > 0 && p[3] == 255),
                "the actual PNG must show the red>green>blue map; the unmapped receiver and sole source are achromatic"
            );
            assert!(weak.upgrade().is_none(), "the completed CPU producer releases encoded maps before publishing a final image");
            drop(running);
            assert!(job.work_released());
            assert!(weak.upgrade().is_none(), "observed final handle/frame cannot pin encoded map backing");
            assert_eq!(host.admitted_bytes(), frame._reservation.bytes + retained + 123);
            job.0.data.lock().unwrap().frame = None;
            drop(frame);
            assert_eq!(host.admitted_bytes(), retained + 123);
            drop(job);
            assert_eq!(host.admitted_bytes(), 123);
            drop(plan);
            assert_eq!(host.admitted_bytes(), 0);
            (png, raw)
        };
        let original = render(None);
        assert_eq!(original, render(Some(0)), "new transfer must preserve actual CPU mapped PNG/raw bytes and sample stream");
        assert_eq!(original, render(Some(4096)), "retired capture scratch must not alter the actual mapped image or retained lease split");
    }

    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn capture_errors_drop_typed_resources_before_outer_credit_and_reject_context_changes() {
        let (scene, image, weak) = mapped();
        let host = cpu(MEMORY_LIMIT);
        let cancel = CancelToken::default();
        let bytes = image.textures.as_ref().unwrap().captured_bytes().unwrap();
        let retained = RetainedMetadata(host.0.shared.budget.reserve(123).unwrap());
        let mut credit =
            host.reserve_capture(bytes, CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact), &cancel, Some(&retained)).unwrap();
        // Observe the exact inner refusal boundary used by the consuming public
        // wrapper: parameters have died, but the enclosing credit is still alive.
        let result = host.prepare_submission_inner(scene, image, JobOptions::default(), None, cancel.clone(), None, Some(&mut credit));
        assert!(matches!(result,Err(e) if e.code==ErrorCode::WrongOwner));
        assert!(weak.upgrade().is_none());
        assert_eq!(host.admitted_bytes(), bytes + 123);
        drop(credit);
        assert_eq!(host.admitted_bytes(), 123);
        drop(retained);
        assert_eq!(host.admitted_bytes(), 0);
        let (scene, image, weak) = mapped();
        let cancel = CancelToken::default();
        let bytes = image.textures.as_ref().unwrap().captured_bytes().unwrap();
        let original = RetainedMetadata(host.0.shared.budget.reserve(123).unwrap());
        let same_sized = RetainedMetadata(host.0.shared.budget.reserve(123).unwrap());
        let mut credit =
            host.reserve_capture(bytes, CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact), &cancel, Some(&original)).unwrap();
        let error =
            host.prepare_submission_inner(scene, image, JobOptions::default(), None, cancel, Some(&same_sized), Some(&mut credit)).err().unwrap();
        assert_eq!(error.code, ErrorCode::WrongOwner, "equal byte count cannot substitute a different retained Plan owner");
        assert!(weak.upgrade().is_none());
        assert_eq!(host.admitted_bytes(), bytes + 246);
        drop(credit);
        drop(original);
        drop(same_sized);
        assert_eq!(host.admitted_bytes(), 0);
        for variant in 0..3 {
            let (scene, mut image, weak) = mapped();
            let cancel = CancelToken::default();
            let bytes = image.textures.as_ref().unwrap().captured_bytes().unwrap();
            let credit = host
                .reserve_capture(
                    if variant == 2 { bytes - 1 } else { bytes },
                    CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact),
                    &cancel,
                    None,
                )
                .unwrap();
            let other = cpu(MEMORY_LIMIT);
            if variant == 1 {
                image.raytrace.as_mut().unwrap().budget = super::super::super::CpuBudget::Studio;
            }
            let owner = if variant == 0 { &other } else { &host };
            let error = owner.prepare_submission_with_capture(scene, image, JobOptions::default(), None, cancel, None, credit).err().unwrap();
            assert_eq!(error.code, if variant == 2 { ErrorCode::LimitExceeded } else { ErrorCode::WrongOwner });
            assert!(weak.upgrade().is_none());
            assert_eq!(host.admitted_bytes(), 0);
            assert_eq!(other.admitted_bytes(), 0);
        }
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn capture_extension_counts_independent_retained_owner_at_exact_cpu_profile_boundary() {
        let host = cpu(MEMORY_LIMIT);
        let cancel = CancelToken::default();
        let plan = RetainedMetadata(host.0.shared.budget.reserve(123).unwrap());
        let maximum = super::super::super::CpuBudget::Compact.limits().max_job_bytes;
        let mut credit =
            host.reserve_capture(maximum - 123, CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact), &cancel, Some(&plan)).unwrap();
        assert_eq!(host.admitted_bytes(), maximum);
        assert_eq!(credit.try_extend(1, &cancel).unwrap_err().code, ErrorCode::LimitExceeded);
        assert_eq!(host.admitted_bytes(), maximum);
        drop(credit);
        assert_eq!(host.admitted_bytes(), 123);
        drop(plan);
        assert_eq!(host.admitted_bytes(), 0);
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn capture_pool_failure_and_cancelled_prepared_keep_credit_until_owned_backing_drops() {
        let (scene, image, weak) = mapped();
        let host = cpu(MEMORY_LIMIT);
        let options = JobOptions::default();
        let admission = host.admit(&scene, &image, options).unwrap();
        let total = admission.estimated_total_bytes + retained_request_bytes(&image).unwrap() + image.retained_resource_metadata_bytes();
        let busy = host.0.shared.budget.reserve(MEMORY_LIMIT - total + 1).unwrap();
        let cancel = CancelToken::default();
        let bytes = image.textures.as_ref().unwrap().captured_bytes().unwrap();
        let mut credit = host.reserve_capture(bytes, CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact), &cancel, None).unwrap();
        let error = host.prepare_submission_inner(scene, image, options, None, cancel, None, Some(&mut credit)).err().unwrap();
        assert_eq!(error.code, ErrorCode::LimitExceeded);
        assert!(weak.upgrade().is_none());
        assert_eq!(host.admitted_bytes(), busy.bytes + bytes);
        drop(credit);
        assert_eq!(host.admitted_bytes(), busy.bytes);
        drop(busy);
        assert_eq!(host.admitted_bytes(), 0);
        let (scene, image, weak) = mapped();
        let cancel = CancelToken::default();
        let admission = host.admit(&scene, &image, options).unwrap();
        let bytes = admission.estimated_total_bytes + retained_request_bytes(&image).unwrap() + image.retained_resource_metadata_bytes() + 4096;
        let credit = host.reserve_capture(bytes, CaptureProfile::CpuRaytrace(super::super::super::CpuBudget::Compact), &cancel, None).unwrap();
        let mut slot = Some(host.prepare_submission_with_capture(scene, image, options, None, cancel.clone(), None, credit).unwrap());
        assert_eq!(slot.as_ref().unwrap().admission().estimated_total_bytes, admission.estimated_total_bytes + 4096);
        let held = host.admitted_bytes();
        assert_eq!(held, bytes);
        cancel.cancel();
        assert_eq!(host.try_submit_prepared(id(9511, 1), 1, &mut slot).err().unwrap().code, ErrorCode::Cancelled);
        assert!(slot.is_some() && weak.upgrade().is_some());
        assert_eq!(host.admitted_bytes(), held, "failed publication retains the full funded capsule");
        drop(slot);
        assert!(weak.upgrade().is_none());
        assert_eq!(host.admitted_bytes(), 0);
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn capture_rechecks_actual_registered_layout_after_admission_before_merging_credit() {
        let (scene, mut image, weak) = mapped();
        image.integrator = Integrator::Raster;
        image.raytrace = None;
        let changed = Arc::new(AtomicU64::new(2));
        let host = registered(MEMORY_LIMIT, changed.clone());
        let cancel = CancelToken::default();
        let bytes = image.textures.as_ref().unwrap().captured_bytes().unwrap();
        let credit = host.reserve_capture(bytes, CaptureProfile::Registered, &cancel, None).unwrap();
        let error = host.prepare_submission_with_capture(scene, image, JobOptions::default(), None, cancel, None, credit).err().unwrap();
        assert_eq!(changed.load(Ordering::Acquire), 3);
        assert_eq!(error.code, ErrorCode::HostUnavailable);
        assert_eq!(error.message, "resolved capture executor metadata changed");
        assert!(weak.upgrade().is_none());
        assert_eq!(host.admitted_bytes(), 0);
        assert!(host.0.shared.queue.lock().unwrap().is_empty());
    }
    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    #[test]
    fn capture_registered_factory_failure_releases_source_credit_and_preserves_only_record_summary() {
        let (scene, mut image, weak) = mapped();
        image.integrator = Integrator::Raster;
        image.raytrace = None;
        let host = registered(MEMORY_LIMIT, Arc::new(AtomicU64::new(0)));
        let cancel = CancelToken::default();
        let retained = image.retained_resource_metadata_bytes();
        let bytes = image.textures.as_ref().unwrap().captured_bytes().unwrap();
        let credit = host.reserve_capture(bytes, CaptureProfile::Registered, &cancel, None).unwrap();
        let prepared = host.prepare_submission_with_capture(scene, image, JobOptions::default(), None, cancel, None, credit).unwrap();
        let job = host.submit_prepared(id(9512, 1), 1, prepared).unwrap();
        assert!(job.0.request.textures.is_none());
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        assert!(Running::begin(work).is_none());
        assert_eq!(job.status().unwrap().progress.phase, Phase::Failed);
        assert!(job.work_released());
        assert!(weak.upgrade().is_none());
        assert!(job.frame().unwrap().is_none());
        assert_eq!(host.admitted_bytes(), retained);
        drop(job);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn actual_cpu_texture_worker_releases_sources_after_completion_or_cancel_while_status_is_retained() {
    use crate::job::textures::{PlanarFrame, RuntimeTextures, TextureBinding, TextureMap};
    use image::ImageEncoder;
    for cancelled in [false, true] {
        // Real native admission and ImageTask, manually pumped to avoid timing races.
        let host = ImageHost(Arc::new(Owner {
            executor: None,
            shared: CpuHost::shared(MEMORY_LIMIT),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }));
        let mut encoded = Vec::new();
        image::codecs::png::PngEncoder::new(&mut encoded).write_image(&[128, 64, 32, 255], 1, 1, image::ExtendedColorType::Rgba8).unwrap();
        let resources = Arc::new(
            RuntimeTextures::new(vec![TextureBinding {
                body_index: 0,
                body_name: "Portable wood".into(),
                frame: PlanarFrame { origin: [0.; 3], u: [1., 0., 0.], v: [0., 1., 0.] },
                normal_strength: 1.,
                base_color: Some(TextureMap::new(encoded, crate::texture_input::MapRole::BaseColor).unwrap()),
                normal: None,
                roughness: None,
                metallic: None,
                base_color_mode: Default::default(),
                flip_v: false,
            }])
            .unwrap(),
        );
        let weak = Arc::downgrade(&resources);
        let mesh = Mesh {
            positions: vec![solidcore_record::Vec3::ZERO, solidcore_record::Vec3::X, solidcore_record::Vec3::Y],
            triangles: vec![[0, 1, 2]],
            ..Default::default()
        };
        let scene = ProductScene {
            bounds: mesh.bounds(),
            meshes: vec![crate::ProductMesh {
                name: "Portable wood".into(),
                mesh: Arc::new(mesh),
                color: [255; 3],
                opacity: 1.,
                surface: None,
                transmission: None,
                faces: vec![],
            }],
        };
        let input = ImageRequest {
            integrator: Integrator::Raytrace,
            raytrace: Some(Default::default()),
            textures: Some(resources),
            width: 2,
            height: 2,
            ..request()
        };
        let job = host.submit(id(1599, 1), 1, scene, input).unwrap();
        let status = job.status().unwrap();
        assert!(status.request.textures.is_none());
        assert_eq!(status.textures.as_ref().unwrap().bodies, 1);
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        let mut running = Running::begin(work).unwrap();
        assert!(weak.upgrade().is_some());
        assert!(running.guarded_step(1));
        if cancelled {
            job.cancel().unwrap();
        }
        let mut finished = false;
        for _ in 0..10_000 {
            if !running.guarded_step(4) {
                finished = true;
                break;
            }
        }
        assert!(finished);
        drop(running);
        assert!(weak.upgrade().is_none(), "terminal CPU status cannot retain encoded texture payloads");
        assert_eq!(job.status().unwrap().progress.phase, if cancelled { Phase::Cancelled } else { Phase::Completed });
        assert_eq!(job.frame().unwrap().is_some(), !cancelled);
        drop(status);
        drop(job);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

// The native worker checks `stop` and starts waiting while it holds the queue lock. A stop sent
// in between must still wake it, else joining the worker waits for good (#14500).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_stop_sent_while_the_worker_is_about_to_wait_still_wakes_it() {
    use std::sync::mpsc::sync_channel;
    use std::time::Duration;
    let shared = ImageHost::shared(MEMORY_LIMIT);
    let (checked, on_checked) = sync_channel(1);
    let (stopped, on_stopped) = sync_channel(1);
    let worker = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            // Hold the lock between the check and the wait, as native_worker does.
            let queue = shared.queue.lock().unwrap();
            assert!(!shared.stop.load(Ordering::Acquire));
            checked.send(()).unwrap();
            // A stop that takes the lock cannot finish here; one that doesn't finishes now.
            let _ = on_stopped.recv_timeout(Duration::from_millis(500));
            // One wait, without checking `stop` again, as native_worker's wait.
            let (_queue, waited) = shared.wake.wait_timeout(queue, Duration::from_secs(10)).unwrap();
            !waited.timed_out()
        })
    };
    on_checked.recv().unwrap();
    let stopper = {
        let shared = Arc::clone(&shared);
        std::thread::spawn(move || {
            shared.stop_worker();
            let _ = stopped.send(());
        })
    };
    assert!(worker.join().unwrap(), "the worker missed the stop and waited until its timeout");
    stopper.join().unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn registered_preview_and_final_profiles_keep_capture_identity_and_completed_mode() {
    struct TwoModes;
    impl ImageExecutor for TwoModes {
        fn descriptor(&self) -> Result<ExecutorDescriptor> {
            let mut descriptor = CpuExecutor.descriptor()?;
            let mut final_image = descriptor.profiles[0].clone();
            final_image.mode = RenderMode::Final;
            descriptor.profiles.push(final_image);
            Ok(descriptor)
        }
        fn admit(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
            CpuExecutor.admit(scene, request, options)
        }
        fn begin(&self, scene: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
            CpuExecutor.begin(scene, request, options, cancel)
        }
    }
    let host = ImageHost::with_executor(Arc::new(TwoModes)).unwrap();
    let cancel = CancelToken::default();
    let credit = host.reserve_capture(0, CaptureProfile::Registered, &cancel, None).unwrap();
    assert_eq!(host.descriptor().unwrap().profiles.len(), 2);
    let mut unsupported = CpuExecutor.descriptor().unwrap();
    unsupported.profiles[0].mode = RenderMode::Quick;
    assert!(validate_image_profiles(&unsupported).is_err());
    drop(credit);
    let mut submission = host.prepare_submission_with_metadata(empty(), request(), JobOptions::default(), None, cancel, None).unwrap();
    assert!(submission.select_image_mode(RenderMode::Quick).is_err());
    assert_eq!(submission.resolved.profile.mode, RenderMode::Preview);
    submission.select_image_mode(RenderMode::Final).unwrap();
    let job = host.submit_prepared(JobId { session: 981, serial: 1, generation: 1 }, 1, submission).unwrap();
    let status = job.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed);
    assert_eq!(status.resolved.profile.mode, RenderMode::Final);
    assert_eq!(status.resolved.executor.as_str(), "cpu-product");
    assert!(job.frame().unwrap().is_some());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn prepared_image_mode_pins_the_final_profile_limits_and_scratch_before_job_creation() {
    struct ChangingDescriptor(Arc<AtomicU64>);
    impl ImageExecutor for ChangingDescriptor {
        fn descriptor(&self) -> Result<ExecutorDescriptor> {
            let mut descriptor = CpuExecutor.descriptor()?;
            let mut final_image = descriptor.profiles[0].clone();
            final_image.mode = RenderMode::Final;
            match self.0.load(Ordering::Acquire) {
                1 => final_image.features.push(crate::RenderFeature::Hdri),
                2 => descriptor.limits.max_dimension -= 1,
                3 => descriptor.scratch.sample_bytes_per_pixel += 1,
                _ => {}
            }
            descriptor.profiles.push(final_image);
            Ok(descriptor)
        }
        fn admit(&self, scene: &ProductScene, request: &ImageRequest, options: JobOptions) -> Result<Admission> {
            CpuExecutor.admit(scene, request, options)
        }
        fn begin(&self, scene: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
            CpuExecutor.begin(scene, request, options, cancel)
        }
    }
    let changed = Arc::new(AtomicU64::new(0));
    let host = ImageHost::with_executor(Arc::new(ChangingDescriptor(changed.clone()))).unwrap();
    for mutation in 1..=3 {
        changed.store(0, Ordering::Release);
        let mut prepared = host.prepare_submission(empty(), request(), JobOptions::default(), None, CancelToken::default()).unwrap();
        let held = host.admitted_bytes();
        changed.store(mutation, Ordering::Release);
        let error = prepared.select_image_mode(RenderMode::Final).unwrap_err();
        assert_eq!(error.code, ErrorCode::HostUnavailable);
        assert!(error.message.contains("descriptor changed"));
        assert_eq!(prepared.resolved.profile.mode, RenderMode::Preview);
        assert!(host.0.shared.queue.lock().unwrap().is_empty());
        assert_eq!(host.admitted_bytes(), held);
        drop(prepared);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native_memory_admission {
    use super::*;
    use crate::job::NativeMemoryPolicy;
    use crate::job::host_memory::{MemorySnapshot, NativeMemoryPool};
    use crate::job::{HostMemorySource, NATIVE_CPU_POOL_BYTES};

    fn host(memory: &Arc<NativeMemoryPool>, limit: u64, registered: bool) -> ImageHost {
        ImageHost(Arc::new(Owner {
            executor: registered.then(|| Arc::new(CpuExecutor) as Arc<dyn ImageExecutor>),
            shared: ImageHost::shared_with_memory(limit, Some(memory.clone())),
            kind: HostKind::NativeWorker,
            running: Mutex::new(None),
            _worker: None,
        }))
    }

    #[test]
    fn platform_cpu_and_registered_constructors_use_the_same_native_memory_pool() {
        let cpu = ImageHost::for_platform().unwrap();
        let registered = ImageHost::with_executor(Arc::new(CpuExecutor)).unwrap();
        assert!(Arc::ptr_eq(cpu.0.shared.budget.memory.as_ref().unwrap(), registered.0.shared.budget.memory.as_ref().unwrap()));
        assert!(cpu.0.shared.budget.memory_admission(0, 0).unwrap().is_some());
        assert_eq!(cpu.admitted_bytes(), 0);
        assert_eq!(registered.admitted_bytes(), 0);
    }

    #[test]
    fn cpu_and_registered_hosts_share_capacity_and_all_lease_transfers() {
        let memory = NativeMemoryPool::testing(100);
        let cpu = host(&memory, 80, false);
        let registered = host(&memory, 80, true);
        let mut cpu_lease = cpu.0.shared.budget.reserve(60).unwrap();
        let error = registered.0.shared.budget.reserve(41).err().unwrap();
        assert_eq!(error.code, ErrorCode::LimitExceeded);
        assert!(error.message.contains("41 additional bytes"), "{}", error.message);
        assert!(error.message.contains("60 bytes already admitted"), "{}", error.message);
        assert!(error.message.contains("40 bytes available"), "{}", error.message);
        assert_eq!(registered.admitted_bytes(), 0);

        // The process ledger can accommodate 30, but this host cannot. Its
        // temporary physical lease must roll back on the software-pool refusal.
        assert!(cpu.0.shared.budget.reserve(30).is_err());
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 60);
        let mut gpu_lease = registered.0.shared.budget.reserve(40).unwrap();
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().available_bytes, 0);
        assert!(cpu_lease.merge_from(&mut gpu_lease).is_err());
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 100);

        let mut checkpoint = cpu_lease.split(20).unwrap();
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 100);
        cpu_lease.shrink(30).unwrap();
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 90);
        cpu_lease.merge_from(&mut checkpoint).unwrap();
        drop(checkpoint);
        assert_eq!(cpu_lease.bytes, 50);
        assert!(cpu_lease.shrink(51).is_err());
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 90);
        drop(gpu_lease);
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 50);
        drop(cpu_lease);
        assert_eq!(cpu.admitted_bytes(), 0);
        assert_eq!(registered.admitted_bytes(), 0);
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 0);
    }

    #[test]
    fn legacy_memory_policy_admits_low_or_zero_ram_without_bypassing_software_limits() {
        for capacity in [0, 32] {
            for registered in [false, true] {
                let memory = NativeMemoryPool::testing(capacity);
                let host = host(&memory, 64, registered);
                assert_eq!(host.native_memory_policy(), NativeMemoryPolicy::AvailableRam);
                assert_eq!(host.0.shared.budget.reserve(48).err().unwrap().code, ErrorCode::LimitExceeded);
                assert_eq!(host.admitted_bytes(), 0);

                host.set_native_memory_policy(NativeMemoryPolicy::SoftwarePoolOnly);
                assert_eq!(host.clone().native_memory_policy(), NativeMemoryPolicy::SoftwarePoolOnly);
                let receipt = host.0.shared.budget.memory_admission(48, 0).unwrap().unwrap();
                assert_eq!(receipt.policy, NativeMemoryPolicy::SoftwarePoolOnly);
                assert_eq!((receipt.required_bytes, receipt.available_bytes), (48, capacity));
                assert_eq!(receipt.software_limit_bytes, NATIVE_CPU_POOL_BYTES);
                assert_eq!(serde_json::to_value(receipt).unwrap()["policy"], "software_pool_only");
                let held = host.0.shared.budget.reserve(48).unwrap();
                assert_eq!(host.admitted_bytes(), 48);
                assert!(host.0.shared.budget.reserve(17).is_err(), "the finite host pool still refuses 65 bytes");
                let snapshot = memory.check(0, 0, NativeMemoryPolicy::SoftwarePoolOnly).unwrap();
                assert_eq!((snapshot.admitted_bytes, snapshot.available_bytes), (48, 0), "legacy allocations stay in the shared ledger");
                drop(held);
                assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 0);
            }
        }
    }

    #[test]
    fn mixed_memory_policies_keep_leases_and_only_change_future_admission() {
        let memory = NativeMemoryPool::testing(100);
        let cpu = host(&memory, 100, false);
        let gpu = host(&memory, 100, true);
        let cpu_lease = cpu.0.shared.budget.reserve(60).unwrap();
        gpu.set_native_memory_policy(NativeMemoryPolicy::SoftwarePoolOnly);
        let receipt = gpu.0.shared.budget.memory_admission(80, 0).unwrap().unwrap();
        let gpu_lease = gpu.0.shared.budget.reserve(80).unwrap();
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::SoftwarePoolOnly).unwrap().admitted_bytes, 140);
        assert!(cpu.0.shared.budget.reserve(1).is_err(), "default hosts account for legacy leases too");
        assert!(gpu.0.shared.budget.reserve(21).is_err(), "legacy mode still rolls back a software-pool refusal");
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::SoftwarePoolOnly).unwrap().admitted_bytes, 140);

        gpu.set_native_memory_policy(NativeMemoryPolicy::AvailableRam);
        assert_eq!(gpu.admitted_bytes(), 80, "changing the policy does not revoke an existing lease");
        assert_eq!(receipt.policy, NativeMemoryPolicy::SoftwarePoolOnly, "an earlier receipt keeps the policy actually used");
        assert!(gpu.0.shared.budget.reserve(1).is_err(), "future reservations use the selected policy");
        drop(cpu_lease);
        let later = cpu.0.shared.budget.reserve(20).unwrap();
        let current = gpu.0.shared.budget.memory_admission(80, 80).unwrap().unwrap();
        assert_eq!(current.policy, NativeMemoryPolicy::AvailableRam);
        assert_eq!((current.required_bytes, current.credited_bytes, current.additional_bytes), (80, 80, 0));
        assert_eq!((current.admitted_bytes, current.physical_debit_bytes, current.available_bytes), (20, 100, 0));
        drop(gpu_lease);
        drop(later);
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 0);

        // This checks only integer accounting, with no image or buffer allocation.
        assert!(memory.reserve(u64::MAX, NativeMemoryPolicy::SoftwarePoolOnly).is_err());
        let first = host(&memory, NATIVE_CPU_POOL_BYTES, false);
        let second = host(&memory, NATIVE_CPU_POOL_BYTES, true);
        first.set_native_memory_policy(NativeMemoryPolicy::SoftwarePoolOnly);
        second.set_native_memory_policy(NativeMemoryPolicy::SoftwarePoolOnly);
        let maximum = first.0.shared.budget.reserve(NATIVE_CPU_POOL_BYTES - 1).unwrap();
        let last = second.0.shared.budget.reserve(1).unwrap();
        let refused = second.0.shared.budget.reserve(1).err().unwrap();
        assert_eq!(refused.code, ErrorCode::LimitExceeded);
        assert!(refused.message.contains("shared software pool"));
        assert_eq!(second.admitted_bytes(), 1, "the refusal must not debit either ledger");
        drop(maximum);
        drop(last);
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 0);
    }

    #[test]
    fn native_snapshot_refreshes_for_settled_owners_but_not_funded_work() {
        let memory = NativeMemoryPool::testing(100);
        let mut old = memory.reserve(60, NativeMemoryPolicy::AvailableRam).unwrap();
        memory.set_test_capacity(20);
        let snapshot = memory.check(40, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((snapshot.capacity_bytes, snapshot.physical_debit_bytes, snapshot.available_bytes), (100, 60, 40));
        let own = memory.check(60, 60, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((own.required_bytes, own.credited_bytes, own.additional_bytes), (60, 60, 0));
        assert_eq!((own.admitted_bytes, own.available_bytes), (0, 40));
        assert!(memory.check(0, 1, NativeMemoryPolicy::AvailableRam).is_err(), "credit cannot exceed the request");
        assert!(memory.check(61, 61, NativeMemoryPolicy::AvailableRam).is_err(), "credit cannot exceed all held leases");

        let activity = memory.activity().unwrap();
        old.settle().unwrap();
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().capacity_bytes, 100, "temporary task backing still pins the epoch");
        drop(activity);
        let smaller = memory.check(20, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((smaller.capacity_bytes, smaller.admitted_bytes, smaller.physical_debit_bytes), (20, 60, 0));
        assert!(memory.reserve(21, NativeMemoryPolicy::AvailableRam).is_err());
        memory.set_test_capacity(200);
        let current = memory.reserve(150, NativeMemoryPolicy::AvailableRam).unwrap();
        assert!(old.split(1).is_err(), "retained backing cannot fund another allocation");
        drop(old);
        let after = memory.check(50, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((after.admitted_bytes, after.physical_debit_bytes, after.available_bytes), (150, 150, 50));
        assert!(memory.reserve(51, NativeMemoryPolicy::AvailableRam).is_err(), "dropping old-epoch backing must not release current debit");
        drop(current);
    }

    #[test]
    fn native_epoch_credit_and_checkpoint_recycling_preserve_new_allocation_debits() {
        let memory = NativeMemoryPool::testing(100);
        let mut old = memory.reserve(10, NativeMemoryPolicy::AvailableRam).unwrap();
        old.settle().unwrap();
        memory.set_test_capacity(80);
        let mut empty = memory.reserve(0, NativeMemoryPolicy::AvailableRam).unwrap();
        let mut current = memory.reserve(40, NativeMemoryPolicy::AvailableRam).unwrap();
        empty.merge_from(&mut current).unwrap();
        drop(current);
        assert!(empty.merge_from(&mut old).is_err(), "old settled credit needs new admission before reuse");
        let mixed = memory.check(70, 50, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((mixed.required_bytes, mixed.credited_bytes, mixed.additional_bytes), (70, 50, 20));
        assert_eq!((mixed.admitted_bytes, mixed.physical_debit_bytes, mixed.available_bytes), (0, 40, 40));
        assert!(memory.check(91, 50, NativeMemoryPolicy::AvailableRam).is_err());
        drop(old);

        let mut free = empty.split(20).unwrap();
        let mut checkpoint = free.split(10).unwrap();
        checkpoint.settle().unwrap();
        free.merge_from(&mut checkpoint).unwrap();
        memory.set_test_capacity(1);
        let pinned = memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((pinned.capacity_bytes, pinned.admitted_bytes, pinned.physical_debit_bytes), (80, 40, 40));
        drop(checkpoint);
        drop(free);
        drop(empty);
        assert_eq!(memory.check(1, 0, NativeMemoryPolicy::AvailableRam).unwrap().capacity_bytes, 1);
    }

    #[test]
    fn native_fallback_keeps_all_epochs_charged_and_epoch_overflow_fails_closed() {
        let memory = NativeMemoryPool::testing(100);
        let mut old = memory.reserve(60, NativeMemoryPolicy::AvailableRam).unwrap();
        old.settle().unwrap();
        memory.set_test_snapshot(MemorySnapshot::from_probe(0, 0, Some(100)));
        let fallback = memory.check(40, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!(fallback.source, HostMemorySource::ConservativeFallback);
        assert_eq!((fallback.admitted_bytes, fallback.physical_debit_bytes, fallback.available_bytes), (60, 60, 40));
        assert!(memory.reserve(41, NativeMemoryPolicy::AvailableRam).is_err());
        let mut legacy = memory.reserve(80, NativeMemoryPolicy::SoftwarePoolOnly).unwrap();
        legacy.settle().unwrap();
        let full = memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((full.admitted_bytes, full.physical_debit_bytes, full.available_bytes), (140, 140, 0));
        assert!(memory.reserve(1, NativeMemoryPolicy::AvailableRam).is_err());
        drop(old);
        let freed = memory.check(20, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((freed.admitted_bytes, freed.physical_debit_bytes, freed.available_bytes), (80, 80, 20));
        drop(legacy);
        assert_eq!(memory.check(100, 0, NativeMemoryPolicy::AvailableRam).unwrap().physical_debit_bytes, 0);

        let exhausted = NativeMemoryPool::testing(100);
        exhausted.set_test_epoch(u64::MAX);
        assert_eq!(exhausted.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap_err().code, ErrorCode::Internal);
        assert!(exhausted.reserve(1, NativeMemoryPolicy::SoftwarePoolOnly).is_err(), "invalid accounting cannot create new credit");
    }

    #[test]
    fn completed_native_frame_and_request_metadata_allow_fresh_ram_after_work_release() {
        let small = request();
        let initial = admit(&empty(), &small).unwrap().estimated_total_bytes + small.retained_resource_metadata_bytes() + 4096;
        let memory = NativeMemoryPool::testing(initial);
        let host = host(&memory, MEMORY_LIMIT, false);
        let job = host.submit(id(7912, 1), 1, empty(), small).unwrap();
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        let mut running = Running::begin(work).unwrap();
        let mut finished = false;
        for _ in 0..10_000 {
            if !running.advance(4096).unwrap() {
                finished = true;
                break;
            }
        }
        assert!(finished);
        let frame = job.frame().unwrap().unwrap();
        assert!(host.admitted_bytes() > frame._reservation.bytes, "retained request metadata must also remain alive");
        let larger = ImageRequest { width: 128, height: 128, ..request() };
        memory.set_test_capacity(MEMORY_LIMIT);
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().capacity_bytes, initial);
        assert!(host.admit(&empty(), &larger, JobOptions::default()).is_err(), "completed Running still owns its lifetime barrier");
        drop(running);
        assert!(job.work_released());
        let admitted = host.admit(&empty(), &larger, JobOptions::default()).unwrap().host_memory.unwrap();
        assert_eq!((admitted.capacity_bytes, admitted.physical_debit_bytes, admitted.available_bytes), (MEMORY_LIMIT, 0, MEMORY_LIMIT));
        assert_eq!(admitted.admitted_bytes, host.admitted_bytes());
        memory.set_test_capacity(admitted.additional_bytes - 1);
        assert!(host.admit(&empty(), &larger, JobOptions::default()).is_err(), "a later downward observation also applies");
        job.0.data.lock().unwrap().frame = None;
        drop(frame);
        assert_eq!(host.admitted_bytes(), fixed_request_metadata_bytes());
        memory.set_test_capacity(2000);
        let metadata_only = memory.check(1500, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((metadata_only.capacity_bytes, metadata_only.physical_debit_bytes), (2000, 0));
        drop(job);
        assert_eq!(host.admitted_bytes(), 0);
    }

    #[cfg(feature = "raytrace-cpu")]
    #[test]
    fn externally_held_native_checkpoint_does_not_pin_the_finished_work_epoch() {
        use crate::job::{CpuProgressiveOptions, RaytraceOptions};
        let memory = NativeMemoryPool::testing(MEMORY_LIMIT);
        let host = host(&memory, MEMORY_LIMIT, false);
        let request = ImageRequest {
            integrator: Integrator::Raytrace,
            raytrace: Some(RaytraceOptions {
                progressive: Some(CpuProgressiveOptions { samples_per_pixel: 3, seed: 7 }),
                lights: Some(Vec::new()),
                ..Default::default()
            }),
            width: 4,
            height: 4,
            ..request()
        };
        let job = host.submit(id(7913, 1), 1, empty(), request).unwrap();
        let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
        let mut running = Running::begin(work).unwrap();
        let mut preview = None;
        let mut finished = false;
        for _ in 0..100_000 {
            if !running.advance(1).unwrap() {
                finished = true;
                break;
            }
            if preview.is_none() {
                preview = job.preview().unwrap();
            }
        }
        assert!(finished);
        let preview = preview.unwrap();
        assert_eq!(preview.info.samples_per_pixel, 1);
        memory.set_test_capacity(MEMORY_LIMIT * 2);
        drop(running);
        let renewed = memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((renewed.capacity_bytes, renewed.physical_debit_bytes), (MEMORY_LIMIT * 2, 0));
        let current = memory.reserve(20, NativeMemoryPolicy::AvailableRam).unwrap();
        drop(preview);
        drop(job);
        let remaining = memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap();
        assert_eq!((remaining.admitted_bytes, remaining.physical_debit_bytes), (20, 20));
        drop(current);
    }

    #[test]
    fn native_probe_distinguishes_zero_capacity_cgroup_bounds_and_unavailable_ram() {
        let fallback = MemorySnapshot::from_probe(0, 0, None);
        assert_eq!(fallback.capacity_bytes, 512 * 1024 * 1024);
        assert_eq!(fallback.source, HostMemorySource::ConservativeFallback);
        assert_eq!(MemorySnapshot::from_probe(0, 0, Some(0)).capacity_bytes, 0);
        let exhausted = MemorySnapshot::from_probe(100, 0, None);
        assert_eq!(exhausted.capacity_bytes, 0);
        assert_eq!(exhausted.source, HostMemorySource::SystemAvailable);
        let cgroup = MemorySnapshot::from_probe(100, 80, Some(30));
        assert_eq!(cgroup.capacity_bytes, 30);
        assert_eq!(cgroup.source, HostMemorySource::SystemAndCgroup);
        assert_eq!(MemorySnapshot::from_probe(100, 80, Some(0)).capacity_bytes, 0);
        assert_eq!(MemorySnapshot::from_probe(100, u64::MAX, None).capacity_bytes, 100);
        assert_eq!(
            MemorySnapshot::from_probe(u64::MAX, u64::MAX, None).capacity_bytes,
            u64::MAX,
            "the RAM observation is separate from the software cap"
        );
    }

    #[test]
    fn native_preflight_reports_capacity_without_reserving_or_creating_an_image() {
        let memory = NativeMemoryPool::testing(MEMORY_LIMIT);
        for registered in [false, true] {
            let host = host(&memory, MEMORY_LIMIT, registered);
            let report = host.admit(&empty(), &request(), JobOptions::default()).unwrap();
            let physical = report.host_memory.unwrap();
            assert_eq!(physical.required_bytes, report.estimated_total_bytes + fixed_request_metadata_bytes());
            assert_eq!(physical.capacity_bytes, MEMORY_LIMIT);
            assert_eq!(physical.admitted_bytes, 0);
            assert_eq!(host.admitted_bytes(), 0);
            assert!(host.0.shared.queue.lock().unwrap().is_empty());
            assert!(report.cpu_workers.is_none(), "raster/registered admission omits CPU worker execution");
        }
    }

    #[cfg(all(feature = "raytrace-cpu", target_pointer_width = "64"))]
    #[test]
    fn maximum_cpu_preflight_uses_real_capacity_gate_without_allocating_film() {
        use crate::job::{CpuBudget, CpuPathOptions, CpuProgressiveOptions, DenoiseSettings, OutputOptions, RaytraceOptions};
        let request = ImageRequest {
            integrator: Integrator::Raytrace,
            raytrace: Some(RaytraceOptions {
                threads: crate::job::MAX_CPU_THREADS,
                budget: CpuBudget::Workstation,
                progressive: Some(CpuProgressiveOptions { samples_per_pixel: crate::job::MAX_UI_SAMPLES, seed: 7 }),
                path: Some(CpuPathOptions { max_depth: crate::job::MAX_PATH_DEPTH }),
                texture_filtering: true,
                metal_energy_compensation: true,
                adaptive: Some(Box::new(crate::job::adaptive::AdaptiveSampling {
                    pilot_samples: crate::job::adaptive::MAX_PILOT_SAMPLES,
                    min_samples: crate::job::MAX_UI_SAMPLES,
                    ..Default::default()
                })),
                caustics: Some(crate::job::CausticOptions {
                    photons: crate::job::MAX_CAUSTIC_PHOTONS,
                    max_depth: crate::job::MAX_PATH_DEPTH,
                    refresh_per_pass: true,
                    ..Default::default()
                }),
                supersampling: 2,
                denoise: true,
                denoise_settings: DenoiseSettings { passes: 3, strength: 1.0, firefly_suppression: true, optical_filter: true },
                denoise_diagnostics: true,
                output: Some(Box::new(OutputOptions {
                    linear_exr: true,
                    finishing: crate::job::finishing::Finishing {
                        bloom: Some(crate::job::bloom::Bloom { strength: 2.0, threshold: 1e6, radius: 64 }),
                        photographic: crate::job::photographic::Photographic {
                            flare: crate::job::photographic::Flare { strength: 2.0, threshold: 1e6, ghosts: 4, halo: 2.0, streak: 2.0 },
                            chromatic_pixels: 12.0,
                            radial_distortion: 0.3,
                            soft_focus: 1.0,
                            sharpening: 1.0,
                            grain: 0.15,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                })),
                ..Default::default()
            }),
            width: 7680,
            height: 4320,
            ..request()
        };
        let layout = crate::job::admit(&empty(), &request).unwrap();
        let required = layout.estimated_total_bytes + retained_request_bytes(&request).unwrap() + request.retained_resource_metadata_bytes();
        assert!(required < CpuBudget::Workstation.limits().max_job_bytes);
        let memory = NativeMemoryPool::testing(required + 1024);
        let cpu = host(&memory, NATIVE_CPU_POOL_BYTES, false);
        let registered = host(&memory, NATIVE_CPU_POOL_BYTES, true);
        let report = cpu.admit(&empty(), &request, JobOptions::default()).unwrap();
        let physical = report.host_memory.unwrap();
        assert_eq!((physical.required_bytes, physical.admitted_bytes, physical.available_bytes), (required, 0, required + 1024));
        assert_eq!(report.cpu_workers, Some(64));
        assert_eq!(report.caustic_work.unwrap().maps, crate::job::MAX_UI_SAMPLES + crate::job::adaptive::MAX_PILOT_SAMPLES);
        assert_eq!(cpu.admitted_bytes(), 0);
        assert!(cpu.0.shared.queue.lock().unwrap().is_empty());
        println!("{}", serde_json::json!({ "maximum_cpu_preflight": report }));

        let competing = registered.0.shared.budget.reserve(1025).unwrap();
        let failure = cpu.admit(&empty(), &request, JobOptions::default()).unwrap_err();
        assert_eq!(failure.code, ErrorCode::LimitExceeded);
        assert!(failure.message.contains(&format!("{required} additional bytes")));
        assert!(failure.message.contains("1025 bytes already admitted"));
        assert!(failure.message.contains(&format!("{} bytes available", required - 1)));
        assert_eq!(cpu.admitted_bytes(), 0);
        drop(competing);
        assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 0);
    }

    #[cfg(feature = "raytrace-cpu")]
    #[test]
    fn prepared_cpu_workers_and_memory_receipt_survive_queueing_and_cancellation() {
        use crate::job::{CpuProgressiveOptions, RaytraceOptions};
        let memory = NativeMemoryPool::testing(MEMORY_LIMIT);
        let host = host(&memory, MEMORY_LIMIT, false);
        for (serial, threads) in [(1, 0), (2, 1), (3, 2)] {
            let request = ImageRequest {
                integrator: Integrator::Raytrace,
                raytrace: Some(RaytraceOptions {
                    threads,
                    progressive: Some(CpuProgressiveOptions { samples_per_pixel: 3, seed: 7 }),
                    lights: Some(Vec::new()),
                    ..Default::default()
                }),
                width: 4,
                height: 4,
                ..request()
            };
            let prepared = host.prepare_submission(empty(), request, JobOptions::default(), None, CancelToken::default()).unwrap();
            let workers = prepared.admission.cpu_workers.unwrap();
            assert!((1..=crate::job::MAX_CPU_THREADS).contains(&workers));
            if threads != 0 {
                assert_eq!(workers, threads);
            }
            assert_eq!(prepared.request().raytrace.as_ref().unwrap().threads, threads);
            let physical = prepared.admission.host_memory.unwrap();
            assert_eq!(physical.required_bytes, host.admitted_bytes());
            assert_eq!(physical.admitted_bytes, 0, "the receipt credits its own already-held lease");
            assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, host.admitted_bytes());
            let handle = host.submit_prepared(id(7910, serial), 1, prepared).unwrap();
            let work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
            assert_eq!(work.cpu_workers, Some(workers));
            handle.cancel().unwrap();
            assert!(Running::begin(work).is_none());
            assert_eq!(handle.status().unwrap().progress.phase, Phase::Cancelled);
            drop(handle);
            assert_eq!(host.admitted_bytes(), 0);
            assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 0);
        }
    }

    #[cfg(feature = "raytrace-cpu")]
    #[test]
    fn queued_cpu_work_refuses_missing_or_reduced_custom_worker_admission() {
        use crate::job::RaytraceOptions;
        for cpu_workers in [None, Some(1), Some(0), Some(65)] {
            let memory = NativeMemoryPool::testing(MEMORY_LIMIT);
            let host = host(&memory, MEMORY_LIMIT, false);
            let request = ImageRequest {
                integrator: Integrator::Raytrace,
                raytrace: Some(RaytraceOptions { threads: 2, ..Default::default() }),
                width: 1,
                height: 1,
                ..request()
            };
            let handle = host.submit(id(7911, 1), 1, empty(), request).unwrap();
            let mut work = host.0.shared.queue.lock().unwrap().pop_front().unwrap();
            assert_eq!(work.cpu_workers, Some(2));
            work.cpu_workers = cpu_workers;
            assert!(Running::begin(work).is_none());
            let failure = handle.status().unwrap().error.unwrap();
            assert_eq!(failure.code, if cpu_workers.is_none() { ErrorCode::Internal } else { ErrorCode::InvalidRequest });
            drop(handle);
            assert_eq!(host.admitted_bytes(), 0);
            assert_eq!(memory.check(0, 0, NativeMemoryPolicy::AvailableRam).unwrap().admitted_bytes, 0);
        }
    }
}
