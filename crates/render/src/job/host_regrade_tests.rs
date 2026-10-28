use super::super::{PngTask, linear_exr::ExrTask, presentation::Presentation};
use super::*;
use std::sync::atomic::AtomicUsize;

struct FixtureExecutor(Arc<AtomicUsize>);
struct FixtureWork {
    png: PngTask,
    exr: Option<Vec<u8>>,
}
impl ImageWork for FixtureWork {
    fn take_linear_exr(&mut self) -> Option<Vec<u8>> {
        self.exr.take()
    }
    fn step(&mut self, units: usize) -> Result<TaskPoll> {
        self.png.step(units)
    }
}
impl ImageExecutor for FixtureExecutor {
    fn supports_linear_exr(&self) -> bool {
        true
    }
    fn descriptor(&self) -> Result<ExecutorDescriptor> {
        CpuExecutor.descriptor()
    }
    fn admit(&self, scene: &ProductScene, request: &ImageRequest, _: JobOptions) -> Result<Admission> {
        let mut a = admit(scene, request)?;
        a.estimated_total_bytes += 1_048_576;
        Ok(a)
    }
    fn begin(&self, _: ProductScene, request: ImageRequest, options: JobOptions, cancel: CancelToken) -> Result<Box<dyn ImageWork>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let pixels: Vec<_> = (0..request.width * request.height).map(|i| if i % 3 == 0 { [4.0, 0.5, 0.125, 0.5] } else { [0.0; 4] }).collect();
        let settings = Presentation { background: request.background, ..Default::default() };
        let mut rgba = vec![0; pixels.len() * 4];
        settings.display(request.width, request.height)?.encode(&pixels, &mut rgba, &cancel)?;
        let exr = if options.linear_exr {
            let mut task = ExrTask::new(request.width, request.height, pixels, cancel.clone())?;
            while !task.step(4)? {}
            Some(task.finish()?.0)
        } else {
            None
        };
        Ok(Box::new(FixtureWork { png: PngTask::new(&request, rgba, cancel)?, exr }))
    }
}
fn fixture(raw: bool) -> (ImageHost, JobHandle, Arc<AtomicUsize>, Presentation) {
    let begins = Arc::new(AtomicUsize::new(0));
    let host = ImageHost::cooperative_with(Arc::new(FixtureExecutor(begins.clone())));
    let request = ImageRequest {
        studio_weights: None,
        width: 16,
        height: 16,
        background: [40, 80, 120],
        transparent_background: false,
        camera: Default::default(),
        integrator: Default::default(),
        raytrace: None,
        textures: None,
        environment: None,
    };
    let settings = Presentation { background: request.background, ..Default::default() };
    let source = host
        .submit_with_options(
            identity(1),
            42,
            ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY },
            request,
            JobOptions { linear_exr: raw, ..Default::default() },
        )
        .unwrap();
    finish(&host, &source);
    (host, source, begins, settings)
}
fn identity(serial: u64) -> JobId {
    JobId { session: 1817, serial, generation: serial }
}
fn finish(host: &ImageHost, handle: &JobHandle) {
    for _ in 0..4096 {
        if !host.pump(4096).unwrap() {
            break;
        }
    }
    let status = handle.status().unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
}

#[test]
fn regrade_is_exact_nondestructive_geometry_free_and_flattens_repeated_master_ownership() {
    let (host, source, begins, settings) = fixture(true);
    let original = source.frame().unwrap().unwrap();
    let raw = original.linear_exr().unwrap().as_ptr();
    let identity = host.submit_regrade(identity(2), &source, settings).unwrap();
    finish(&host, &identity);
    let neutral = identity.frame().unwrap().unwrap();
    assert_eq!(neutral.png(), original.png());
    assert_eq!(neutral.linear_exr().unwrap().as_ptr(), raw);
    assert_eq!(neutral.info.scene_generation, 42);
    assert_eq!(neutral.info.executor.as_str(), "linear-master");
    let origin = identity.status().unwrap().presentation.unwrap();
    assert_eq!(origin.source_job, source.id());
    assert_eq!(origin.master_job, source.id());
    assert_eq!(origin.master_sha256, original.linear_master_info().unwrap().sha256);
    assert!(!origin.denoise_applied);
    let changed = Presentation {
        exposure_ev: -2.0,
        finishing: super::super::finishing::Finishing {
            saturation: 0.2,
            bloom: Some(super::super::bloom::Bloom { strength: 0.4, radius: 2, threshold: 1.0 }),
            ..Default::default()
        },
        backdrop: Some(Default::default()),
        ..settings
    };
    let derivative = host.submit_regrade(super::regrade_tests::identity(3), &identity, changed).unwrap();
    finish(&host, &derivative);
    let frame = derivative.frame().unwrap().unwrap();
    assert_ne!(frame.png(), original.png());
    assert_eq!(frame.linear_exr().unwrap().as_ptr(), raw);
    assert!(Arc::ptr_eq(&frame.master_source().unwrap(), &original));
    assert_eq!(frame.presentation().unwrap().source_job, identity.id());
    assert_eq!(begins.load(Ordering::Relaxed), 1, "post-hoc presentation may never launch an executor");
    drop(source);
    drop(identity);
    drop(derivative);
    drop(neutral);
    drop(original);
    assert!(host.admitted_bytes() > frame.png().len() as u64, "raw source and origin metadata leases remain live");
    drop(frame);
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn regrade_rejects_foreign_missing_or_invalid_masters_and_cancellation_never_publishes() {
    let (host, no_raw, _, settings) = fixture(false);
    let before = host.admitted_bytes();
    assert!(host.submit_regrade(identity(2), &no_raw, settings).is_err());
    assert_eq!(host.admitted_bytes(), before);
    let (host, source, begins, settings) = fixture(true);
    let before = host.admitted_bytes();
    assert!(host.submit_regrade(JobId { session: 7, ..identity(2) }, &source, settings).is_err());
    assert!(host.submit_regrade(source.id(), &source, settings).is_err());
    assert!(host.submit_regrade(identity(2), &source, Presentation { exposure_ev: f64::NAN, ..settings }).is_err());
    assert_eq!(host.admitted_bytes(), before);
    for (serial, begin_work) in [(3, false), (4, true)] {
        let job = host.submit_regrade(identity(serial), &source, settings).unwrap();
        if begin_work {
            assert!(host.pump(1).unwrap());
        }
        job.cancel().unwrap();
        for _ in 0..64 {
            if !host.pump(1).unwrap() {
                break;
            }
        }
        assert_eq!(job.status().unwrap().progress.phase, Phase::Cancelled);
        assert!(job.frame().unwrap().is_none());
        drop(job);
        assert_eq!(host.admitted_bytes(), before);
    }
    assert_eq!(begins.load(Ordering::Relaxed), 1);
    drop(source);
    assert_eq!(host.admitted_bytes(), 0);
}
