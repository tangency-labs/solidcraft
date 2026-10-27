use super::*;
use crate::scenario::Harness;
use solidcraft_engine::Session;

#[cfg(not(target_arch = "wasm32"))]
mod async_path_host {
    use super::*;
    use solidcraft_engine::render::job;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Advertise the path capability without opening a device or rendering anything.
    struct TestExecutor;
    impl job::ImageExecutor for TestExecutor {
        fn descriptor(&self) -> job::Result<ExecutorDescriptor> {
            let mut descriptor = job::CpuHost::cooperative().descriptor()?;
            descriptor.id = ExecutorId::new("test-path").unwrap();
            descriptor.profiles[0].features.push(solidcraft_engine::render::RenderFeature::Progressive);
            Ok(descriptor)
        }
        fn admit(&self, _: &ProductScene, _: &job::ImageRequest, _: job::JobOptions) -> job::Result<job::Admission> {
            panic!("setup tests must not render")
        }
        fn begin(&self, _: ProductScene, _: job::ImageRequest, _: job::JobOptions, _: job::CancelToken) -> job::Result<Box<dyn job::ImageWork>> {
            panic!("setup tests must not render")
        }
    }

    fn panel_fixture() -> Harness {
        let mut h = Harness::new();
        h.call("ui.resize", json!({"width":1600,"height":1600}));
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.frames(3);
        h
    }

    fn finish_setup(h: &mut Harness) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while matches!(h.app.render_workspace.path_host, PathHost::Preparing(..)) {
            assert!(Instant::now() < deadline, "test setup worker did not finish");
            h.frames(1);
            std::thread::sleep(Duration::from_millis(1));
        }
        h.frames(3);
    }

    #[test]
    fn design_frames_and_document_reset_never_start_the_deferred_path_host() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut h = Harness::new();
        h.app.render_workspace.defer_path_host("selected viewport".into(), move || {
            observed.fetch_add(1, Ordering::SeqCst);
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        h.frames(5);
        crate::documents::adopt(&mut h.app, Session::default());
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
        assert!(h.app.render_workspace.pending_setup.is_some());
        enter(&mut h.app, true).unwrap();
        finish_setup(&mut h);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        enter(&mut h.app, false).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn first_render_before_adapter_install_starts_once_when_the_adapter_arrives() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut h = panel_fixture();
        assert!(matches!(h.app.render_workspace.path_host, PathHost::NotStarted));
        h.app.render_workspace.defer_path_host("late selected viewport".into(), move || {
            observed.fetch_add(1, Ordering::SeqCst);
            Err("deliberate device refusal".into())
        });
        sync_backend(&mut h.app);
        finish_setup(&mut h);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Failed(_)));
        h.app.render_workspace.defer_path_host("must not retry".into(), || panic!("failed setup retried"));
        enter(&mut h.app, false).unwrap();
        enter(&mut h.app, true).unwrap();
        h.frames(5);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_fallback_viewport_replaces_an_unstarted_factory_without_opening_the_old_device() {
        let mut h = Harness::new();
        h.app.render_workspace.defer_path_host("old hardware adapter".into(), || panic!("superseded device opened"));
        h.app.render_workspace.defer_path_host("selected safe-mode adapter".into(), || Err("selected adapter unavailable".into()));
        h.frames(5);
        assert_eq!(h.app.render_workspace.setup_key.as_deref(), Some("selected safe-mode adapter"));
        enter(&mut h.app, true).unwrap();
        finish_setup(&mut h);
        let PathHost::Failed(message) = &h.app.render_workspace.path_host else { panic!("expected selected adapter result") };
        assert_eq!(message, "selected adapter unavailable");
    }

    #[test]
    fn preparing_keeps_frames_responsive_then_registers_ready_host() {
        let mut h = panel_fixture();
        let cpu_host = h.app.session.render_job_host().unwrap().clone();
        let (release, wait) = mpsc::channel();
        let (started, thread) = mpsc::channel();
        h.app.render_workspace.path_host.start(move || {
            started.send(std::thread::current().id()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        assert_ne!(thread.recv_timeout(Duration::from_secs(10)).unwrap(), std::thread::current().id());
        // A second viewport installation cannot replace or duplicate setup.
        h.app.render_workspace.path_host.start(|| panic!("setup started twice"));
        h.frames(3);
        assert!(h.widget("GPU path tracer: compiling shaders…").is_some());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
        assert!(!h.app.session.render_backend().executors.iter().any(|e| e.id.as_str() == "test-path"));
        let transparent = h.app.tr("render.image_transparent").to_string();
        h.step(&json!({"widget":transparent})).unwrap();
        assert!(h.app.render_workspace.view.transparent);
        // Replacing the current document must retain the receiver while setup waits.
        crate::documents::adopt(&mut h.app, Session::default());
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Preparing(..)));
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.frames(3);
        assert!(h.widget("GPU path tracer: compiling shaders…").is_some());
        enter(&mut h.app, false).unwrap();
        release.send(()).unwrap();
        finish_setup(&mut h);
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.frames(3);
        assert!(h.widget("GPU path tracer: compiling shaders…").is_none());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_ok());
        assert_eq!(h.app.session.render_backend().executors.iter().filter(|e| e.id.as_str() == "test-path").count(), 1);
        assert!(h.app.session.render_job_host().unwrap().same_host(&cpu_host));
        h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
        h.step(&json!({"click":{"handle":"render:method-gpu"}})).unwrap();
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
        crate::documents::adopt(&mut h.app, Session::default());
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Ready(_)));
        assert_eq!(h.app.session.render_backend().executors.iter().filter(|e| e.id.as_str() == "test-path").count(), 1);
    }

    /// The first setup on an adapter shows a spinner, the time so far and a note; a later one
    /// a bar estimated from the time the first took, which the preferences keep.
    #[test]
    fn setup_progress_is_estimated_from_the_last_setup_on_the_adapter() {
        let mut h = panel_fixture();
        h.app.render_workspace.setup_key = Some("Test GPU | Vulkan | 1.0".into());
        let (release, wait) = mpsc::channel::<()>();
        h.app.render_workspace.path_host.start(move || {
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        h.frames(3);
        let first = h.app.tr("render.gpu_path_first_setup").to_string();
        assert!(h.widget("GPU path tracer: compiling shaders…").is_some());
        assert!(h.widget(&first).is_some(), "the first setup explains the wait");
        assert!(h.widget("0:00").is_some(), "and counts the time");
        release.send(()).unwrap();
        finish_setup(&mut h);
        let took = h.app.render_workspace.setup_seconds.get("Test GPU | Vulkan | 1.0").copied();
        assert!(took.is_some_and(|s| (0.0..10.0).contains(&s)), "{took:?}");

        // The next start on the same adapter has an estimate, kept in the preferences.
        let mut h = panel_fixture();
        h.app.load_prefs(&json!({"gpu_path_setup_seconds": {"Test GPU | Vulkan | 1.0": 120.0}}).to_string());
        h.app.render_workspace.setup_key = Some("Test GPU | Vulkan | 1.0".into());
        let (release, wait) = mpsc::channel::<()>();
        h.app.render_workspace.path_host.start(move || {
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            job::ImageHost::with_executor(Arc::new(TestExecutor)).map_err(|e| e.to_string())
        });
        h.frames(3);
        assert!(h.widget(&first).is_none());
        assert!(h.widget("0:00 / ~2:00").is_some(), "a bar with the estimate");
        release.send(()).unwrap();
        finish_setup(&mut h);
        let prefs: serde_json::Value = serde_json::from_str(&h.app.prefs()).unwrap();
        assert!(prefs["gpu_path_setup_seconds"]["Test GPU | Vulkan | 1.0"].as_f64().is_some_and(|s| s < 10.0));
    }

    #[test]
    fn hostile_setup_times_are_ignored() {
        let times = load_setup_seconds(&json!({"a": -1.0, "b": 1e300, "c": "slow", "d": 30.0}));
        assert_eq!(times.into_iter().collect::<Vec<_>>(), [("d".to_string(), 30.0)]);
        let many: serde_json::Map<String, serde_json::Value> = (0..100).map(|i| (format!("gpu {i}"), json!(1.0))).collect();
        assert_eq!(load_setup_seconds(&serde_json::Value::Object(many)).len(), MAX_SETUP_TIMES);
    }

    #[test]
    fn setup_error_is_visible_and_survives_document_reset() {
        let mut h = panel_fixture();
        h.app.render_workspace.path_host.start(|| Err("test driver refused the device".into()));
        finish_setup(&mut h);
        let message = "GPU path tracer unavailable: test driver refused the device";
        assert!(h.widget(message).is_some());
        assert!(h.widget("GPU path tracer: compiling shaders…").is_none());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
        assert!(h.app.session.render_job_host().is_some());
        completed_preview(&mut h, [200, 210, 220]);
        crate::documents::adopt(&mut h.app, Session::default());
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.frames(3);
        assert!(h.widget(message).is_some());
        h.app.render_workspace.path_host.start(|| panic!("failed setup retried on viewport install"));
        assert!(matches!(h.app.render_workspace.path_host, PathHost::Failed(_)));
    }

    #[test]
    fn disconnected_setup_is_reported_in_the_panel() {
        let mut h = panel_fixture();
        let (tx, rx) = mpsc::channel();
        h.app.render_workspace.path_host = PathHost::Preparing(rx, Instant::now());
        drop(tx);
        h.frames(3);
        assert!(h.widget("GPU path tracer unavailable: setup worker stopped without returning a host").is_some());
    }

    #[test]
    fn panicking_setup_is_reported_in_the_panel() {
        let mut h = panel_fixture();
        h.app.render_workspace.path_host.start(|| panic!("simulated driver panic"));
        finish_setup(&mut h);
        assert!(h.widget("GPU path tracer unavailable: GPU path tracer setup panicked").is_some());
    }

    #[test]
    fn registration_failure_is_reported_in_the_panel() {
        let mut h = panel_fixture();
        // A different host with the same CPU descriptor cannot be registered twice.
        h.app.render_workspace.path_host.start(|| Ok(job::CpuHost::cooperative()));
        finish_setup(&mut h);
        let PathHost::Failed(message) = &h.app.render_workspace.path_host else { panic!("duplicate executor must fail") };
        assert!(h.widget(&format!("GPU path tracer unavailable: {message}")).is_some());
        assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn opaque_pbr_product_gpu_and_cpu_share_color_and_material_semantics() {
    use solidcraft_engine::render::{
        StandardView,
        job::{CancelToken, ImageRequest, ImageTask, TaskPoll},
    };
    let mut s = Session::default();
    s.execute("solid.sphere", &json!({"radius":10,"body_name":"Original sphere"})).unwrap();
    const SIZE: u32 = 256;
    let mut previous = None;
    for (metallic, roughness) in [(0.0, 0.2), (0.0, 0.65), (1.0, 0.2), (1.0, 0.65)] {
        s.execute(
            "appearance.assign",
            &json!({"bodies":["Original sphere"],"color":"#b47846",
            "pbr":{"metallic":metallic,"roughness":roughness,"dielectric_ior":1.5}}),
        )
        .unwrap();
        let scene = solidcraft_engine::product_scene::extract(&s).unwrap();
        let (key, gpu) = gpu_scene(&scene).unwrap();
        assert!(!gpu.pbr.is_empty() && gpu.tris.is_empty());
        assert_ne!(previous, Some(key));
        previous = Some(key);
        let mut camera = Camera { fov: 0.0, ..Default::default() };
        camera.set_view(StandardView::Iso);
        camera.fit(&scene.bounds);
        let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target);
        let eye = camera.eye().to_f32();
        let mut task = ImageTask::new(
            scene,
            ImageRequest {
                environment: None,
                textures: None,
                camera,
                width: SIZE,
                height: SIZE,
                background: [255; 3],
                transparent_background: false,
                integrator: Default::default(),
                raytrace: None,
            },
            CancelToken::default(),
        )
        .unwrap();
        let mut png = None;
        for _ in 0..100_000 {
            if let TaskPoll::Complete(bytes) = task.step(4096).unwrap() {
                png = Some(bytes);
                break;
            }
        }
        let cpu = image::load_from_memory(&png.expect("bounded original fixture completes")).unwrap().to_rgba8();
        assert_eq!(cpu.dimensions(), (SIZE, SIZE));
        let cpu = cpu.into_raw();
        for srgb in [false, true] {
            let Some(gpu) = crate::gpu::render_product_offscreen(
                gpu.clone(),
                camera.view_proj(1.0, radius).to_f32(),
                camera.back().to_f32(),
                [eye[0], eye[1], eye[2], 0.0],
                SIZE,
                srgb,
            ) else {
                eprintln!("skipping: no GPU adapter");
                return;
            };
            let at = |pixels: &[u8], x: usize, y: usize| {
                let i = (y * SIZE as usize + x) * 4;
                [pixels[i], pixels[i + 1], pixels[i + 2]]
            };
            let mut core = 0usize;
            let mut error = 0usize;
            let mut outliers = 0usize;
            for y in 2..SIZE as usize - 2 {
                for x in 2..SIZE as usize - 2 {
                    // Exclude antialiased silhouettes: CPU uses 2x supersampling, GPU uses 1x.
                    if (y - 1..=y + 1).any(|ny| (x - 1..=x + 1).any(|nx| at(&cpu, nx, ny) == [255; 3] || at(&gpu, nx, ny) == [255; 3])) {
                        continue;
                    }
                    let delta = at(&cpu, x, y).into_iter().zip(at(&gpu, x, y)).map(|(a, b)| usize::from(a.abs_diff(b))).collect::<Vec<_>>();
                    error += delta.iter().sum::<usize>();
                    outliers += usize::from(delta.iter().any(|d| *d > 12));
                    core += 1;
                }
            }
            assert!(core > 1000);
            let mean = error as f64 / (3 * core) as f64;
            eprintln!("PBR m={metallic} r={roughness} srgb={srgb}: core={core}, mean={mean}, outliers={outliers}");
            assert!(mean < 2.0 && outliers * 100 < core * 2, "CPU/GPU color or material interpretation differs");
        }
    }
}

#[test]
fn control_render_export_respects_upstream_file_scope() {
    let mut h = Harness::new();
    let root = h.dir.join("allowed");
    let outside = h.dir.join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let roots = json!([root.to_string_lossy()]);
    let response = h.call("ui.workspace", json!({"workspace":"render","allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    let response = h.call("engine.execute", json!({"command":"render.start","params":{"settings":{"width":16,"height":16}},"allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    let job = response["result"]["job"].clone();
    for _ in 0..50 {
        h.frames(1);
    }
    assert_eq!(h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"], "completed");
    for path in [outside.join("denied.png"), root.join("../outside/traversal.png")] {
        for overwrite in [false, true] {
            let response = h.call(
                "engine.execute",
                json!({"command":"render.export","params":{"job":job,"path":path,"overwrite":overwrite},"allow_paths":roots}),
            );
            assert_eq!(response["ok"], false, "{response}");
            assert!(response.to_string().contains("outside the folders this agent may use"), "{response}");
        }
    }
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);
    let path = root.join("allowed.png");
    let response = h.call("engine.execute", json!({"command":"render.export","params":{"job":job,"path":path},"allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    assert!(path.is_file());
    let original = std::fs::read(&path).unwrap();
    let response = h.call("engine.execute", json!({"command":"render.export","params":{"job":job,"path":path,"overwrite":true},"allow_paths":roots}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn adopting_into_blank_document_resets_render_draft_and_preserves_hosts() {
    let mut h = Harness::new();
    h.app.render_workspace.host = Some(host_executor(2048, true, 4).unwrap());
    sync_backend(&mut h.app);
    h.app.render_workspace.view.name = "Old draft".into();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    let mut incoming = Session::default();
    incoming.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    crate::documents::adopt(&mut h.app, incoming);
    assert!(!active(&h.app));
    assert_eq!(h.app.render_workspace.view.name, "Studio");
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
    assert!(h.app.session.render_job_host().is_some());
    assert_eq!(h.app.session.model.state().bodies.len(), 1);
}

#[test]
fn clicked_image_controls_complete_cancel_and_export_without_dirtying_the_design() {
    for language in ["en", "de", "ko", "ru"] {
        let mut h = Harness::new();
        h.app.preferences.language = language.into();
        h.frames(1);
        let check = h.app.tr("cmd.render_preflight").to_string();
        let start = h.app.tr("render.image_start").to_string();
        let export = h.app.tr("render.image_export").to_string();
        let cancel = h.app.tr("cmd.render_cancel").to_string();
        h.call("ui.resize", json!({"width":1600,"height":1400}));
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.app.render_workspace.view.draft.width = 16;
        h.app.render_workspace.view.draft.height = 16;
        h.app.render_workspace.view.dirty = true;
        h.frames(3);
        let doc = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        h.step(&json!({"widget":check})).unwrap();
        h.step(&json!({"widget":start})).unwrap();
        for _ in 0..50 {
            h.frames(1);
            if h.app.render_workspace.view.completed.is_some() {
                break;
            }
        }
        let good = h.app.render_workspace.view.completed.unwrap();
        h.step(&json!({"widget":export})).unwrap();
        assert!(h.dir.join("render.png").is_file());
        h.app.render_workspace.view.draft.width = 1024;
        h.app.render_workspace.view.draft.height = 768;
        h.step(&json!({"widget":start})).unwrap();
        h.step(&json!({"widget":cancel})).unwrap();
        h.frames(5);
        assert_eq!(h.app.render_workspace.view.completed, Some(good));
        assert_eq!(
            h.app.session.execute("render.status", &json!({"job":h.app.render_workspace.view.job})).unwrap()["progress"]["phase"],
            "cancelled"
        );
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert!(!h.app.session.is_dirty());
    }
}

#[test]
fn saved_camera_survives_frames_and_product_fit_ignores_selection() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Product"})).unwrap();
    h.app.run("solid.box", json!({"corner":[100,0,0],"length":10,"width":20,"height":5,"body_name":"Other"})).unwrap();
    let camera = Camera { up: solidcraft_engine::render::UpAxis::Y, roll: 0.2, ..Default::default() };
    h.app.run("render.settings.set", json!({"name":"Studio","set":{"camera":camera,"background":[21,43,65]}})).unwrap();
    h.frames(3);
    let design = h.app.cam;
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.frames(3);
    h.step(&json!({"widget":"Restore Saved Camera"})).unwrap();
    h.frames(3);
    assert_eq!(h.app.cam, camera);
    assert_eq!(h.app.render_workspace.view.draft.background, [21, 43, 65]);
    h.app.session.selection = vec![solidcraft_engine::Sel::Body { name: "Product".into() }];
    let bounds = solidcraft_engine::product_scene::extract(&h.app.session).unwrap().bounds;
    let mut expected = camera;
    expected.fit(&bounds);
    h.call("engine.execute", json!({"command":"view.fit","params":{}}));
    h.frames(3);
    assert_eq!(h.app.cam, expected);
    expected.set_view(solidcraft_engine::render::StandardView::Iso);
    expected.fit(&bounds);
    h.call("engine.execute", json!({"command":"view.home","params":{}}));
    assert_eq!(h.app.cam, expected);
    enter(&mut h.app, false).unwrap();
    assert_eq!(h.app.cam, design);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn progressive_image_controls_route_to_the_registered_host() {
    let host = solidcraft_gpu::select::Policy::from_env()
        .and_then(|policy| solidcraft_gpu::select::pick(policy).ok_or_else(|| "no qualifying adapter".to_string()))
        .and_then(|(adapter, _)| solidcraft_gpu::path::host_on_adapter(&adapter).map_err(|e| e.to_string()));
    let host = match host {
        Ok(host) => host,
        Err(e) => {
            let vulkan = egui_wgpu::wgpu::Backends::VULKAN;
            let expected = solidcraft_gpu::select::native_backends().contains(vulkan);
            assert!(!expected || std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "required GPU unavailable: {e}");
            eprintln!("skipping progressive UI device test: {e}");
            return;
        }
    };
    let mut h = Harness::new();
    h.app.render_workspace.path_host = PathHost::Ready(host);
    sync_backend(&mut h.app);
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.app.session.mark_saved();
    h.call("ui.resize", json!({"width":1600,"height":1600}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.render_workspace.view.draft.width = 16;
    h.app.render_workspace.view.draft.height = 16;
    h.app.render_workspace.view.samples = 4;
    h.app.render_workspace.view.max_depth = 3;
    h.app.render_workspace.view.seed = 7;
    h.app.render_workspace.view.draft.exposure_ev = 0.5;
    h.app.render_workspace.view.studio.tone_map = solidcraft_engine::render::job::ToneMap::AcesFitted;
    h.app.render_workspace.view.studio.lens_enabled = true;
    h.app.render_workspace.view.studio.lens.aperture_radius_mm = 0.0;
    h.app.render_workspace.view.studio.lighting_enabled = true;
    h.app.render_workspace.view.studio.ambient = [0.75; 3];
    // The UI request reaches the real registered host with a coat and captured map.
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let mut material = materials::Binding::new(target, &h.app);
    material.coat_enabled = true;
    material.maps_enabled = true;
    let map_path = h.dir.join("original-color.png");
    image::RgbaImage::from_pixel(1, 1, image::Rgba([80, 160, 220, 255])).save(&map_path).unwrap();
    material.maps.color = map_path.to_string_lossy().into_owned();
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings.push(material);
    h.frames(3);
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:method-gpu"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    let checked = image_params(&h.app).unwrap();
    assert_eq!(checked["raytrace"]["clear_coats"][0]["weight"], 1.0);
    assert_eq!(checked["raytrace"]["textures"][0]["base_color_srgb"], map_path.to_string_lossy().as_ref());
    h.app.run("render.preflight", checked.clone()).unwrap();
    assert_eq!(image_params(&h.app).unwrap(), checked);
    let start = h.app.tr("render.image_start").to_string();
    h.step(&json!({"widget":start})).unwrap();
    let job = h.app.render_workspace.view.job.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while h.app.render_workspace.view.completed.is_none() {
        assert!(std::time::Instant::now() < deadline, "progressive UI job did not complete");
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.frames(1);
    }
    let status = h.app.session.execute("render.status", &json!({"job":job})).unwrap();
    assert_eq!(status["frame"]["executor"], "gpu-path", "{status}");
    assert_eq!(status["options"]["samples"], 4);
    assert_eq!(status["options"]["max_depth"], 3);
    assert_eq!(status["options"]["seed"], 7);
    assert_eq!(status["options"]["exposure_ev"], 0.5);
    assert_eq!(status["options"]["tone_map"], "aces_fitted");
    assert_eq!(status["options"]["lens"]["aperture_radius_mm"], 0.0);
    assert_eq!(status["options"]["lighting"]["environment"], json!([0.75, 0.75, 0.75]));
    assert_eq!(status["textures"]["bodies"], 1);
    let export = h.app.tr("render.image_export").to_string();
    h.step(&json!({"widget":export})).unwrap();
    assert!(h.dir.join("render.png").is_file());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
    // A pack glass recipe follows the same production request and passes actual
    // closed-mesh preparation. It never calls scalar appearance assignment.
    let material = &mut h.app.render_workspace.view.materials.bindings[0];
    material.coat_enabled = false;
    material.maps_enabled = false;
    material.glass_enabled = true;
    material.glass = materials::Glass::recipe("starter.glass.clear-soda-lime").unwrap();
    let checked = image_params(&h.app).unwrap();
    h.app.run("render.preflight", checked.clone()).unwrap();
    assert_eq!(image_params(&h.app).unwrap(), checked);
    let started = h.app.run("render.start", checked).unwrap();
    let job = serde_json::from_value(started["job"].clone()).unwrap();
    let status = h.app.session.render_job_handle(job).unwrap().wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_engine::render::job::Phase::Completed, "{:?}", status.error);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn clicked_render_workspace_saves_one_setup_and_restores_design_view() {
    for language in ["en", "de", "ko", "ru"] {
        let mut h = Harness::new();
        h.app.preferences.language = language.into();
        h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
        h.app.session.mark_saved();
        h.frames(3);
        let camera = h.app.cam;
        let flags = (h.app.ui.show_grid, h.app.origin_shown(), h.app.ui.visual_style, h.app.ui.perspective);
        let undo = h.app.session.undo.len();
        let at = crate::scenario::rect_of("workspace_switcher").unwrap().center();
        h.step(&json!({"click":[at.x,at.y]})).unwrap();
        h.step(&json!({"click":{"handle":"menu:ui.workspace.render"}})).unwrap();
        assert!(active(&h.app));
        let heading = h.app.tr("render.studio_title").to_string();
        let perspective = h.app.tr("render.perspective").to_string();
        let save = h.app.tr("render.save_setup").to_string();
        let back = h.app.tr("render.return_design").to_string();
        assert!(h.widget(&heading).is_some(), "{language}");
        h.step(&json!({"click":{"handle":"render:page:camera"}})).unwrap();
        h.step(&json!({"widget":perspective})).unwrap();
        assert!(!h.app.session.is_dirty(), "preview edits are staged");
        h.step(&json!({"widget":save})).unwrap();
        assert_eq!(h.app.session.undo.len(), undo + 1);
        assert_eq!(h.app.session.doc.render.setups["Studio"].camera, h.app.cam);
        h.step(&json!({"widget":back})).unwrap();
        assert!(!active(&h.app));
        assert_eq!(h.app.cam, camera);
        assert_eq!(flags, (h.app.ui.show_grid, h.app.origin_shown(), h.app.ui.visual_style, h.app.ui.perspective));
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(!h.app.session.is_dirty());
    }
}

#[test]
fn render_drafts_and_capabilities_are_isolated_across_document_switches() {
    let mut h = Harness::new();
    h.app.render_workspace.host = Some(host_executor(2048, true, 4).unwrap());
    sync_backend(&mut h.app);
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
    h.app.render_workspace.view.name = "First draft".into();
    h.app.render_workspace.view.draft.background = [20, 40, 60];
    h.app.render_workspace.view.dirty = true;
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert!(!active(&h.app));
    assert_eq!(h.app.render_workspace.view.name, "Studio");
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
    crate::documents::switch(&mut h.app, 0);
    assert_eq!(h.app.render_workspace.view.name, "First draft");
    assert_eq!(h.app.render_workspace.view.draft.background, [20, 40, 60]);
    assert!(h.app.render_workspace.view.dirty);
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
    h.app.run("file.new", json!({})).unwrap();
    assert_eq!(h.app.render_workspace.view.name, "Studio");
    assert_eq!(h.app.session.render_backend().executors.len(), 3);
}

#[test]
fn product_gpu_buffers_preserve_opacity_and_never_contain_overlays() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Glass"})).unwrap();
    s.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#336699","opacity":0.4})).unwrap();
    let scene = solidcraft_engine::product_scene::extract(&s).unwrap();
    let (key, gpu) = gpu_scene(&scene).unwrap();
    assert!(gpu.tris.is_empty());
    assert!(!gpu.glass.is_empty());
    let color = |v: &[u8]| {
        let i = v.get(crate::gpu::TRI_SIZE - 4..).and_then(|b| b.try_into().ok()).map(u32::from_le_bytes)?;
        gpu.palette.get(crate::gpu::PaletteIndex(i))
    };
    assert!(gpu.glass.as_chunks::<{ crate::gpu::TRI_SIZE }>().0.iter().all(|v| color(v) == Some([51, 102, 153, 102])));
    assert!(gpu.lines.is_empty() && gpu.overlay.is_empty() && gpu.under.is_empty() && gpu.hidden.is_empty() && gpu.ghost.is_empty());
    s.selection = vec![solidcraft_engine::Sel::Body { name: "Glass".into() }];
    s.section = Some((solidcraft_engine::geom::Vec3::ZERO, solidcraft_engine::geom::Vec3::X));
    let after = solidcraft_engine::product_scene::extract(&s).unwrap();
    assert_eq!(gpu_scene(&after).unwrap().0, key);
    s.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#336699","opacity":0.0})).unwrap();
    let invisible = solidcraft_engine::product_scene::extract(&s).unwrap();
    assert_eq!(invisible.meshes[0].opacity, 0.0);
    let (_, empty) = gpu_scene(&invisible).unwrap();
    assert!(empty.tris.is_empty() && empty.glass.is_empty());
}

#[test]
fn transmission_uses_legacy_preview_buffers_and_invalidates_the_opaque_cache() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10,"body_name":"Glass"})).unwrap();
    let appearance = json!({"bodies":["Glass"],"color":"#336699",
        "pbr":{"metallic":0,"roughness":0.1,"dielectric_ior":1.5}});
    s.execute("appearance.assign", &appearance).unwrap();
    let (opaque_key, opaque) = gpu_scene(&solidcraft_engine::product_scene::extract(&s).unwrap()).unwrap();
    assert!(!opaque.pbr.is_empty());
    let mut scene = solidcraft_engine::product_scene::extract(&s).unwrap();
    scene.meshes[0].transmission =
        Some(solidcraft_engine::render::transmission::Transmission::new(solidcraft_engine::geom::Vec3::ZERO, None).unwrap());
    let (glass_key, glass) = gpu_scene(&scene).unwrap();
    assert_ne!(opaque_key, glass_key);
    assert!(glass.pbr.is_empty());
    assert!(!glass.tris.is_empty());
    scene.meshes[0].opacity = 0.4;
    let (_, transparent) = gpu_scene(&scene).unwrap();
    assert!(transparent.pbr.is_empty());
    assert!(!transparent.glass.is_empty());
}

#[test]
fn host_profiles_advertise_only_installed_opaque_pbr_not_hdri_refraction_or_final() {
    use solidcraft_engine::render::*;
    let host = host_executor(512, true, 4).unwrap();
    assert_eq!(host.limits.max_dimension, 512);
    assert_eq!(host.profiles.len(), 1);
    assert_eq!(host.profiles[0].features, vec![RenderFeature::Pbr]);
    assert!(host.profiles[0].formats.is_empty());
    let mut s = Session::default();
    s.set_render_viewport(Some(host)).unwrap();
    let mut request = RenderRequest {
        backend: None,
        mode: RenderMode::Preview,
        operation: RenderOperation::Viewport,
        width: 512,
        height: 512,
        format: None,
        features: Vec::new(),
        policy: Default::default(),
    };
    assert_eq!(resolve_request(&[s.render_backend()], &request).unwrap().executor.as_str(), "gpu-viewport");
    request.features = vec![RenderFeature::Pbr];
    assert!(resolve_request(&[s.render_backend()], &request).is_ok());
    for unsupported in [RenderFeature::Hdri, RenderFeature::Refraction, RenderFeature::Progressive] {
        request.features = vec![unsupported];
        assert!(resolve_request(&[s.render_backend()], &request).is_err());
    }
    request.features.clear();
    request.width = 513;
    assert!(resolve_request(&[s.render_backend()], &request).is_err());
    request.width = 512;
    request.mode = RenderMode::Final;
    assert!(resolve_request(&[s.render_backend()], &request).is_err());
    assert!(host_executor(1024, false, 1).unwrap().availability != Availability::Available);
    assert!(host_executor(1024, true, 0).is_err());
}

fn completed_preview(h: &mut Harness, background: [u8; 3]) -> solidcraft_engine::render::job::JobId {
    let reply = h.app.session.execute("render.start", &json!({"settings":{"width":16,"height":16,"background":background}})).unwrap();
    let job = serde_json::from_value(reply["job"].clone()).unwrap();
    let host = h.app.session.render_job_host().unwrap().clone();
    for _ in 0..100 {
        host.pump(16384).unwrap();
        if h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"] == "completed" {
            return job;
        }
    }
    panic!("small cooperative image did not complete within the work budget");
}

fn image_panel_frame(app: &mut SolveApp, ctx: &egui::Context) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(285.0, 1200.0))), ..Default::default() },
        |ui| image_controls(app, ui),
    )
}

#[test]
fn completed_image_is_painted_once_available_and_upload_is_cached() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    crate::fonts::setup_bundled_fonts(&ctx);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert!(h.app.render_workspace.image.is_none());
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    let mut output = image_panel_frame(&mut h.app, &ctx);
    let (cached_job, texture) = h.app.render_workspace.image.as_ref().unwrap();
    let texture = texture.as_ref().unwrap();
    let id = texture.id();
    assert_eq!(*cached_job, job);
    assert_eq!(texture.size(), [16, 16]);
    let uploaded = &output.textures_delta.set.get(&id).unwrap().first().unwrap().image;
    let egui::ImageData::Color(pixels) = uploaded;
    assert!(pixels.pixels.iter().all(|p| *p == Color32::from_rgb(20, 40, 60)));
    // egui requires frame texture deltas to be applied or cleared.
    output.textures_delta.clear();
    assert!(
        ctx.tessellate(output.shapes, output.pixels_per_point)
            .iter()
            .any(|shape| { matches!(&shape.primitive, egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == id && !mesh.indices.is_empty()) })
    );
    let mut output = image_panel_frame(&mut h.app, &ctx);
    assert!(!output.textures_delta.set.contains_key(&id), "repaint must reuse the texture");
    // egui requires frame texture deltas to be applied or cleared.
    output.textures_delta.clear();
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let path = h.dir.join("displayed.png");
    h.app.run("render.export", json!({"job":job,"path":path})).unwrap();
    let frame = h.app.session.render_job_handle(job).unwrap().frame().unwrap().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png(), "displaying the image must not change export bytes");
}

#[test]
fn completed_image_keeps_last_export_during_work_and_updates_for_the_next_job() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    crate::fonts::setup_bundled_fonts(&ctx);
    let first = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(first);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    let old_texture = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let reply = h.app.run("render.start", json!({"settings":{"width":16,"height":16}})).unwrap();
    let pending = serde_json::from_value(reply["job"].clone()).unwrap();
    h.app.render_workspace.view.job = Some(pending);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().0, first);
    h.app.run("render.cancel", json!({"job":pending})).unwrap();
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().0, first);
    let second = completed_preview(&mut h, [80, 100, 120]);
    h.app.render_workspace.view.job = Some(second);
    let mut output = image_panel_frame(&mut h.app, &ctx);
    let (cached_job, texture) = h.app.render_workspace.image.as_ref().unwrap();
    assert_eq!(*cached_job, second);
    assert_ne!(texture.as_ref().unwrap().id(), old_texture);
    assert!(output.textures_delta.free.contains(&old_texture));
    // egui requires frame texture deltas to be applied or cleared.
    output.textures_delta.clear();
    h.app.render_workspace.view.result = true;
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    h.app.render_workspace.view.result_camera.distance = 42.0;
    let framing = h.app.render_workspace.view.result_camera;
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert!(h.app.render_workspace.image.is_none(), "another document must not show this image");
    assert!(!h.app.render_workspace.view.result);
    crate::documents::switch(&mut h.app, 0);
    image_panel_frame(&mut h.app, &ctx).textures_delta.clear();
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().0, second);
    assert!(h.app.render_workspace.view.result);
    assert_eq!(h.app.render_workspace.view.result_camera, framing);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    reset_document(&mut h.app);
    assert!(h.app.render_workspace.image.is_none());
    assert!(!h.app.render_workspace.view.result);
}

#[test]
fn completed_image_decoder_preserves_alpha_and_rejects_invalid_sizes_and_pngs() {
    let mut png = std::io::Cursor::new(Vec::new());
    let pixels = image::RgbaImage::from_pixel(2, 1, image::Rgba([80, 120, 160, 128]));
    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let decoded = decode_image(png.get_ref(), 2, 1).unwrap();
    assert_eq!(decoded.size, [2, 1]);
    assert_eq!(decoded.pixels, vec![Color32::from_rgba_unmultiplied(80, 120, 160, 128); 2]);
    for (width, height) in [(0, 1), (u32::MAX, 1), (4096, 4096), (1, 1), (3, 1)] {
        assert!(decode_image(png.get_ref(), width, height).is_err(), "{width}x{height}");
    }
    assert!(decode_image(b"invalid PNG", 2, 1).is_err());
}

#[test]
fn cpu_checkpoints_replace_only_the_provisional_texture_and_keep_final_export_and_framing() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    let first = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.completed = Some(first);
    h.app.render_workspace.view.job = Some(first);
    cache_render_images(&mut h.app, &ctx);
    let final_texture = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let second = completed_preview(&mut h, [80, 100, 120]);
    let frame = h.app.session.render_job_handle(second).unwrap().frame().unwrap().unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.render_workspace.view.job = Some(second);
    let mut info = PreviewInfo { frame_sequence: 1, samples_per_pixel: 1, frame: frame.info.clone() };
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    remember_displayed_job(&mut h.app, false);
    assert_eq!(displayed_image(&h.app).unwrap().info.job, second);
    assert!(displayed_image(&h.app).unwrap().preview.is_some());
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    h.app.render_workspace.view.result_camera.distance = 123.0;
    h.app.render_workspace.view.result_camera.target.x = 5.0;
    let camera = h.app.render_workspace.view.result_camera;
    let first_checkpoint = h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id();
    // Repeated or older metadata never re-decodes bytes, even if passed bad data.
    cache_preview_frame(&mut h.app, &ctx, &info, b"must not decode again");
    assert_eq!(h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id(), first_checkpoint);
    info.frame_sequence = 2;
    info.samples_per_pixel = 2;
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    let newer = h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id();
    assert_ne!(newer, first_checkpoint);
    info.frame_sequence = 1;
    cache_preview_frame(&mut h.app, &ctx, &info, b"out of order");
    remember_displayed_job(&mut h.app, false);
    assert_eq!(h.app.render_workspace.preview_image.as_ref().unwrap().info.frame_sequence, 2);
    assert_eq!(h.app.render_workspace.preview_image.as_ref().unwrap().image.as_ref().unwrap().id(), newer);
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id(), final_texture);
    assert_eq!(h.app.render_workspace.view.completed, Some(first));
    let path = h.dir.join("checkpoint-export-is-final.png");
    h.app.run("render.export", json!({"job":h.app.render_workspace.view.completed,"path":path})).unwrap();
    let retained = h.app.session.render_job_handle(first).unwrap().frame().unwrap().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), retained.png());
    // A final can arrive after the panel read a still-running status. The cached
    // completed ID deliberately remains the old job until publication is sampled.
    cache_render_images(&mut h.app, &ctx);
    assert!(h.app.render_workspace.preview_image.is_none());
    assert!(displayed_image(&h.app).unwrap().preview.is_none());
    assert_eq!(h.app.render_workspace.image_info.as_ref().unwrap().job, second);
    assert_eq!(h.app.render_workspace.view.completed, Some(second));
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    cache_render_images(&mut h.app, &ctx);
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free, "a subsequent status refresh must not refit the new final");
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
}

#[test]
fn cancelling_a_preview_returns_to_the_final_image_without_resetting_pan_or_export() {
    let mut h = Harness::new();
    let ctx = egui::Context::default();
    let final_job = completed_preview(&mut h, [30, 50, 70]);
    h.app.render_workspace.view.completed = Some(final_job);
    h.app.render_workspace.view.job = Some(final_job);
    cache_render_images(&mut h.app, &ctx);
    let frame = h.app.session.render_job_handle(final_job).unwrap().frame().unwrap().unwrap();
    let reply = h.app.run("render.start", json!({"settings":{"width":16,"height":16}})).unwrap();
    let pending = serde_json::from_value(reply["job"].clone()).unwrap();
    h.app.render_workspace.view.job = Some(pending);
    let mut info = PreviewInfo { frame_sequence: 1, samples_per_pixel: 1, frame: frame.info.clone() };
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    assert!(h.app.render_workspace.preview_image.is_none(), "a different job's checkpoint must be ignored");
    info.frame.job = pending;
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    remember_displayed_job(&mut h.app, false);
    h.app.render_workspace.view.result_scale = ResultScale::Free;
    h.app.render_workspace.view.result_camera.target.x = 12.0;
    h.app.render_workspace.view.result_camera.distance = 87.0;
    let camera = h.app.render_workspace.view.result_camera;
    h.app.run("render.cancel", json!({"job":pending})).unwrap();
    cache_render_images(&mut h.app, &ctx);
    assert!(h.app.render_workspace.preview_image.is_none());
    assert_eq!(displayed_image(&h.app).unwrap().info.job, final_job);
    assert_eq!(h.app.render_workspace.view.result_camera, camera);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    assert_eq!(h.app.render_workspace.view.completed, Some(final_job));
    assert!(h.app.run("render.export", json!({"job":pending,"path":h.dir.join("not-final.png")})).is_err());
    let path = h.dir.join("after-preview-cancel.png");
    h.app.run("render.export", json!({"job":final_job,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png());
    // Document adoption must also release a provisional texture without leaking it into a tab.
    cache_preview_frame(&mut h.app, &ctx, &info, frame.png());
    assert!(h.app.render_workspace.preview_image.is_some());
    h.app.render_workspace.view.raytrace.progressive = Some(Default::default());
    crate::documents::open_tab(&mut h.app, Session::default()).unwrap();
    assert!(displayed_image(&h.app).is_none());
    assert!(h.app.render_workspace.preview_image.is_none());
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    ctx.run_ui(Default::default(), |_| {}).textures_delta.clear();
}

#[test]
fn result_framing_fits_both_axes_and_actual_size_respects_display_pixels() {
    let canvas = egui::Rect::from_min_size(egui::pos2(40.0, 80.0), egui::vec2(800.0, 500.0));
    for size in [[1920, 1080], [100, 4000], [16, 16]] {
        for dpi in [1.0, 1.5, 2.0] {
            let fit = result_camera(size, canvas, dpi, ResultScale::Fit);
            assert!(fit.is_valid());
            let fitted = result_rect(fit, size, canvas);
            assert!(canvas.contains_rect(fitted), "{size:?} at {dpi}: {fitted:?}");
            assert_eq!(fitted.center(), canvas.center());
            let actual = result_rect(result_camera(size, canvas, dpi, ResultScale::Actual), size, canvas);
            assert!((actual.width() * dpi - size[0] as f32).abs() < 0.01);
            assert!((actual.height() * dpi - size[1] as f32).abs() < 0.01);
        }
    }
}

#[test]
fn result_canvas_paints_large_capture_reports_staleness_and_preserves_export() {
    let mut h = Harness::new();
    let ctx = crate::fonts::test_context();
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    h.app.render_workspace.view.completed = Some(job);
    h.app.render_workspace.view.result = true;
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    let preview = h.app.cam;
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let frame = h.app.session.render_job_handle(job).unwrap().frame().unwrap().unwrap();
    let draw = |app: &mut SolveApp| {
        let mut out = ctx.run_ui(
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() },
            |ui| show(app, ui),
        );
        out.textures_delta.clear();
        out
    };
    let out = draw(&mut h.app);
    let id = h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id();
    let current = h.app.tr("render.result_current");
    assert!(crate::test_support::text_shapes(&out).any(|t| t.galley.text() == current));
    let identity = format!("{}:{}:{}", job.session, job.serial, job.generation);
    assert!(crate::test_support::text_shapes(&out).any(|t| t.galley.text().contains(&identity)));
    let image_bounds = ctx
        .tessellate(out.shapes, out.pixels_per_point)
        .iter()
        .filter_map(|p| match &p.primitive {
            egui::epaint::Primitive::Mesh(mesh) if mesh.texture_id == id => {
                Some(mesh.vertices.iter().fold(egui::Rect::NOTHING, |r, v| r.union(egui::Rect::from_min_max(v.pos, v.pos))))
            }
            _ => None,
        })
        .fold(egui::Rect::NOTHING, |a, b| a.union(b));
    assert!(image_bounds.width() > 240.0 && image_bounds.height() > 240.0, "{image_bounds:?}");
    assert_eq!(h.app.cam, preview);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.session.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    let out = draw(&mut h.app);
    assert!(crate::test_support::text_shapes(&out).any(|t| t.galley.text() == h.app.tr("render.image_stale")));
    assert_eq!(h.app.render_workspace.image_info.as_ref().unwrap().scene_generation, frame.info.scene_generation);
    assert_eq!(h.app.render_workspace.image.as_ref().unwrap().1.as_ref().unwrap().id(), id);
    let path = h.dir.join("result-canvas.png");
    h.app.run("render.export", json!({"job":job,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png());
}

#[test]
fn result_navigation_uses_shared_pan_zoom_and_leaves_product_camera_unchanged() {
    use egui::{Event, Key, Modifiers, PointerButton, vec2};
    let mut h = Harness::new();
    let ctx = crate::fonts::test_context();
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.completed = Some(job);
    h.app.render_workspace.view.result = true;
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.ui.perspective = true;
    h.app.preferences.smooth_wheel_zoom = false;
    h.app.preferences.zoom_reverse = false;
    let preview = h.app.cam;
    let draft = serde_json::to_value(&h.app.render_workspace.view.draft).unwrap();
    let dirty = h.app.render_workspace.view.dirty;
    let document = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let mut time = 0.0;
    nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![]);
    let fitted = h.app.render_workspace.view.result_camera;
    let p = h.app.viewport.rect.unwrap().center() + vec2(80.0, 20.0);
    nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![Event::PointerMoved(p)]);
    nav_frame(
        &mut h.app,
        &ctx,
        true,
        &mut time,
        Modifiers::NONE,
        vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, 3.0),
            modifiers: Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        }],
    );
    assert!(h.app.render_workspace.view.result_camera.distance < fitted.distance);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Free);
    let before_pan = h.app.render_workspace.view.result_camera.target;
    let space = |pressed| Event::Key { key: Key::Space, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    nav_frame(
        &mut h.app,
        &ctx,
        true,
        &mut time,
        Modifiers::NONE,
        vec![space(true), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }],
    );
    nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![Event::PointerMoved(p + vec2(40.0, -20.0))]);
    nav_frame(
        &mut h.app,
        &ctx,
        true,
        &mut time,
        Modifiers::NONE,
        vec![
            Event::PointerButton { pos: p + vec2(40.0, -20.0), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
            space(false),
        ],
    );
    assert_ne!(h.app.render_workspace.view.result_camera.target, before_pan);
    assert_eq!(h.app.render_workspace.view.result_camera.pitch, fitted.pitch);
    assert_eq!(h.app.render_workspace.view.result_camera.yaw, fitted.yaw);
    let before_key = h.app.render_workspace.view.result_camera.target;
    assert!(crate::navigation::view_key(&mut h.app, &ctx, Key::ArrowRight, Modifiers::CTRL, false));
    assert_ne!(h.app.render_workspace.view.result_camera.target, before_key);
    assert!(!crate::navigation::view_key(&mut h.app, &ctx, Key::ArrowRight, Modifiers::NONE, false));
    for command in ["view.fit", "view.home"] {
        h.app.render_workspace.view.result_camera.target.x += 20.0;
        h.app.run(command, json!({})).unwrap();
        nav_frame(&mut h.app, &ctx, true, &mut time, Modifiers::NONE, vec![]);
        assert_eq!(h.app.render_workspace.view.result_camera, fitted);
    }
    assert_eq!(h.app.cam, preview);
    assert!(h.app.ui.perspective);
    assert_eq!(serde_json::to_value(&h.app.render_workspace.view.draft).unwrap(), draft);
    assert_eq!(h.app.render_workspace.view.dirty, dirty);
    assert!(Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert_eq!(image_params(&h.app).unwrap()["settings"]["camera"], json!(preview));
}

#[test]
fn clicked_result_controls_switch_canvas_and_frame_without_changing_the_setup() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":1400}));
    let job = completed_preview(&mut h, [20, 40, 60]);
    h.app.render_workspace.view.job = Some(job);
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.frames(3);
    let camera = h.app.cam;
    let doc = h.app.session.doc.clone();
    let result = h.app.tr("render.result_title").to_string();
    h.step(&json!({"widget":result})).unwrap();
    h.frames(2);
    assert!(h.app.render_workspace.view.result);
    h.step(&json!({"widget":"100%"})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Actual);
    let fit = h.app.tr("render.result_fit").to_string();
    h.step(&json!({"widget":fit})).unwrap();
    h.frames(2);
    assert_eq!(h.app.render_workspace.view.result_scale, ResultScale::Fit);
    let preview = h.app.tr("render.result_preview").to_string();
    h.step(&json!({"widget":preview})).unwrap();
    assert!(!h.app.render_workspace.view.result);
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
}

#[test]
fn image_method_requests_never_fall_back_or_mix_cpu_and_gpu_options() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.render_workspace.view.result = true;
    h.app.render_workspace.view.result_camera.distance = h.app.cam.distance * 3.0;
    h.app.render_workspace.view.raytrace.exposure_ev = f64::NAN;
    h.app.render_workspace.view.samples = 256;
    h.app.render_workspace.view.max_depth = 12;
    h.app.render_workspace.view.seed = 42;
    h.app.render_workspace.view.draft.exposure_ev = 0.0;
    h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
    h.app.render_workspace.view.studio.hdr = hdr::Input::FilePending { path: "never-opened.hdr".into() };
    h.app.render_workspace.view.studio.denoise = true;
    h.app.render_workspace.view.studio.lens_enabled = true;
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.hdr = hdr::Input::FilePending { path: "never-opened-cpu.hdr".into() };
    let params = image_params(&h.app).unwrap();
    assert_eq!(params["settings"]["camera"], json!(h.app.cam));
    assert_eq!(params["settings"]["exposure_ev"], 0.0);
    for key in ["progressive", "samples", "max_depth", "seed", "integrator", "raytrace", "environment", "lighting", "tone_map", "lens", "denoise"] {
        assert!(params.get(key).is_none(), "raster must omit {key}");
    }
    assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    assert!(image_params(&h.app).is_err(), "missing GPU cannot silently render on CPU");
    h.app.render_workspace.view.method = ImageMethod::CpuRaytrace;
    assert!(image_params(&h.app).is_err(), "unsupported CPU options or host must be refused");
    h.app.render_workspace.view.method = ImageMethod::Raster;
    h.app.session.set_render_job_host(None).unwrap();
    assert!(image_params(&h.app).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_cpu_controls_check_and_start_the_same_capture_and_keep_it_exportable() {
    use solidcraft_engine::render::job::{CpuHost, Phase};
    let mut h = Harness::new();
    let host = CpuHost::for_platform().unwrap();
    h.app.session.set_render_job_host(Some(host.clone())).unwrap();
    h.app.render_workspace.image_host = Some(host);
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.render_workspace.view.draft.width = 16;
    h.app.render_workspace.view.draft.height = 16;
    h.app.render_workspace.view.dirty = true;
    h.frames(3);
    let available = image_host(&h.app, ImageMethod::CpuRaytrace).is_ok();
    h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:method-cpu"}})).unwrap();
    if !available {
        assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
        assert!(image_params(&h.app).is_err());
        return;
    }
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::CpuRaytrace);
    h.step(&json!({"click":{"handle":"render:aa"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:aa-4"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 4);
    h.app.render_workspace.view.raytrace.preset = Some(LightingPreset::WarmCool);
    h.app.render_workspace.view.draft.exposure_ev = 1.0;
    let numeric = image_params(&h.app).unwrap();
    assert_eq!(numeric["settings"]["job"]["method"], "cpu_raytrace");
    assert!(numeric.get("raytrace").is_none());
    let numeric_check = h.app.tr("cmd.render_preflight").to_string();
    h.step(&json!({"widget":numeric_check})).unwrap();
    assert_eq!(h.app.render_workspace.view.cpu.checked.as_ref().unwrap().request, numeric);
    let reflected = h.app.session.world_state().bodies.first().unwrap().name.clone();
    let reflection_section = format!("dialog:section:{}", h.app.tr("render.cpu_reflections"));
    h.step(&json!({"click":{"handle":reflection_section}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-reflection-body"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:list:{reflected}")}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-reflection-add"}})).unwrap();
    h.step(&json!({"click":{"handle":format!("render:cpu-reflection-mode:{reflected}")}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:list:render.cpu_reflection_mirror"}})).unwrap();
    assert_eq!(
        h.app.render_workspace.view.raytrace.reflections.as_ref().unwrap().bodies[0].mode,
        solidcraft_engine::render::job::ReflectionMode::Mirror
    );
    h.step(&json!({"click":{"handle":reflection_section}})).unwrap();
    let hdr_path = h.dir.join("cpu-captured.hdr");
    let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 1 +X 2\n".to_vec();
    hdr.extend_from_slice(&[128, 128, 128, 129, 128, 128, 128, 129]);
    std::fs::write(&hdr_path, &hdr).unwrap();
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    hdr::inspect(&mut h.app, hdr::Target::Cpu, hdr_path.to_string_lossy().into_owned()).unwrap();
    h.app.render_workspace.view.cpu.samples = 4;
    h.app.render_workspace.view.result = true;
    h.app.render_workspace.view.result_camera.distance = 42.0;
    h.frames(2);
    let camera = h.app.cam;
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let params = image_params(&h.app).unwrap();
    let preflight = h.app.session.execute("render.preflight", &params).unwrap();
    assert_eq!(preflight["resolved"]["executor"], "cpu-raytrace");
    let check = h.app.tr("cmd.render_preflight").to_string();
    h.step(&json!({"widget":check})).unwrap();
    assert_eq!(image_params(&h.app).unwrap(), params);
    let checked = h.app.render_workspace.view.cpu.checked.as_ref().unwrap();
    assert_eq!(checked.request, params);
    assert_eq!(checked.environment, preflight["environment"]);
    let start = h.app.tr("render.image_start").to_string();
    h.step(&json!({"widget":start})).unwrap();
    let id = h.app.render_workspace.view.job.unwrap();
    let handle = h.app.session.render_job_handle(id).unwrap();
    // The job owns the source bytes captured by Start, even if the file changes.
    std::fs::write(&hdr_path, b"changed after capture").unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(json!(status.environment), preflight["environment"]);
    assert_eq!(status.progress.phase, Phase::Completed);
    assert_eq!(status.request.integrator, Integrator::Raytrace);
    assert_eq!(json!(status.request.camera), params["settings"]["camera"]);
    assert_eq!(json!(status.request.raytrace), params["raytrace"]);
    assert_eq!(status.request.width, 16);
    assert_eq!(status.request.height, 16);
    h.frames(3);
    assert_eq!(h.app.render_workspace.view.completed, Some(id));
    assert_eq!(h.app.cam, camera);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let frame = handle.frame().unwrap().unwrap();
    assert_eq!(frame.info.executor.as_str(), "cpu-raytrace");
    let path = h.dir.join("cpu-result-canvas.png");
    h.app.run("render.export", json!({"job":id,"path":path})).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), frame.png());
    h.app.render_workspace.view.transparent = true;
    assert!(image_params(&h.app).unwrap_err().contains(h.app.tr("render.cpu_opaque_required")));
    h.app.render_workspace.view.transparent = false;
    h.app.render_workspace.view.raytrace.primary_samples = 3;
    assert!(image_params(&h.app).is_err());
    h.app.render_workspace.view.raytrace.primary_samples = 4;
    h.app.render_workspace.view.raytrace.area_lights = Some(Vec::new());
    assert_eq!(image_params(&h.app).unwrap()["raytrace"]["area_lights"], json!([]));
}

fn async_export_fixture() -> (Harness, solidcraft_engine::render::job::JobId) {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5})).unwrap();
    h.app.session.mark_saved();
    let job = completed_preview(&mut h, [200, 210, 220]);
    (h, job)
}

#[test]
fn delayed_image_export_holds_its_document_without_changing_history() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("delayed.png");
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(h.app.file_picker_busy() && !path.exists());
    assert_eq!(h.call("ui.inspect", json!({}))["result"]["file_dialog_open"], true);
    crate::documents::new_design(&mut h.app);
    let other = h.app.session.doc.clone();
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert_eq!(h.app.docs.active, 0, "upstream holds tab changes while a picker is pending");
    assert_eq!(h.call("ui.inspect", json!({}))["result"]["file_dialog_open"], false);
    assert!(Arc::ptr_eq(&other, &h.app.session.doc));
    assert_eq!(&std::fs::read(&path).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    crate::documents::switch(&mut h.app, 0);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn delayed_image_export_cancel_and_overwrite_choice_preserve_existing_files() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("existing.png");
    std::fs::write(&path, b"existing contents").unwrap();
    let doc = h.app.session.doc.clone();
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    *slot.borrow_mut() = Some(None);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!h.app.file_picker_busy());
    assert_eq!(std::fs::read(&path).unwrap(), b"existing contents");
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    h.app.render_workspace.view.overwrite = true;
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert_eq!(std::fs::read(&path).unwrap(), b"existing contents", "a later checkbox change is not overwrite consent for the pending export");
    assert!(matches!(&h.app.status, Some((_, _, true))));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.execute("render.status", &json!({"job":job})).unwrap()["progress"]["phase"], "completed");
}

#[test]
fn delayed_image_export_never_substitutes_a_newer_retained_image() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("old-image.png");
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    let newer = completed_preview(&mut h, [20, 30, 40]);
    h.app.render_workspace.view.completed = Some(newer);
    *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!path.exists(), "the evicted requested image must fail instead of exporting the newer image");
    assert!(matches!(&h.app.status, Some((_, _, true))));
    assert_eq!(h.app.render_workspace.view.completed, Some(newer));
    let newer_path = h.dir.join("new-image.png");
    h.app.run("render.export", json!({"job":newer,"path":newer_path})).unwrap();
    assert!(newer_path.is_file());
}

#[test]
fn delayed_image_export_holds_close_and_new_document_until_cancel() {
    use crate::picker_test_support::{Slot, save};
    let (mut h, job) = async_export_fixture();
    let path = h.dir.join("closed.png");
    let ctx = crate::fonts::test_context();
    let slot: Slot = Default::default();
    save(&mut h.app, &slot);
    export_image(&mut h.app, job, false);
    crate::file_picker::tick(&mut h.app, &ctx);
    crate::documents::new_design(&mut h.app);
    crate::documents::close(&mut h.app, 0, true);
    let other = h.app.session.doc.clone();
    assert_eq!(h.app.docs.count(), 1);
    assert!(h.app.file_picker_busy());
    *slot.borrow_mut() = Some(None);
    crate::file_picker::tick(&mut h.app, &ctx);
    assert!(!path.exists() && !h.app.file_picker_busy());
    assert!(Arc::ptr_eq(&other, &h.app.session.doc));
}

#[test]
fn delayed_image_export_refuses_an_internally_replaced_document_even_with_overwrite() {
    use crate::picker_test_support::{Slot, save};
    for overwrite in [false, true] {
        let (mut h, job) = async_export_fixture();
        let path = h.dir.join("keep.png");
        std::fs::write(&path, b"keep this file").unwrap();
        let slot: Slot = Default::default();
        save(&mut h.app, &slot);
        export_image(&mut h.app, job, overwrite);
        assert!(h.app.run("appearance.assign", json!({"bodies":["Body1"],"color":[1,2,3]})).is_err());
        // Internal host/session changes are outside the input hold; never export them by accident.
        h.app.session.execute("appearance.assign", &json!({"bodies":["Body1"],"color":[1,2,3]})).unwrap();
        let changed = h.app.session.doc.clone();
        let history = (h.app.session.undo.len(), h.app.session.revision);
        *slot.borrow_mut() = Some(Some(path.to_string_lossy().into_owned()));
        crate::file_picker::tick(&mut h.app, &crate::fonts::test_context());
        assert_eq!(std::fs::read(path).unwrap(), b"keep this file");
        assert!(Arc::ptr_eq(&changed, &h.app.session.doc));
        assert_eq!((h.app.session.undo.len(), h.app.session.revision), history);
        assert!(matches!(&h.app.status, Some((_, _, true))));
    }
}

/// One frame of either the modelling viewport or the Render preview, over the same rectangle.
fn nav_frame(app: &mut crate::SolveApp, ctx: &egui::Context, render: bool, time: &mut f64, mods: egui::Modifiers, events: Vec<egui::Event>) {
    *time += 1.0 / 60.0;
    let events = std::iter::once(egui::Event::ModifiersChanged(mods)).chain(events).collect();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
        time: Some(*time),
        focused: true,
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        if render {
            show(app, ui);
        } else {
            crate::viewport::show(app, ui);
        }
    });
    out.textures_delta.clear();
}

fn nav_app(render: bool, preset: &str, around: crate::prefs::OrbitAround) -> crate::SolveApp {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":40,"width":30,"height":20,"body_name":"Product"})).unwrap();
    let mut app = crate::SolveApp::new(s, crate::Services::default());
    if render {
        enter(&mut app, true).unwrap();
        assert!(active(&app));
    }
    app.preferences.nav = preset.into();
    app.preferences.orbit_around = around;
    app.preferences.zoom_to_cursor = true;
    app.ui.perspective = true;
    let mut cam = Camera::default();
    cam.set_view(solidcraft_engine::render::StandardView::Iso);
    cam.fit(&solidcraft_engine::product_scene::extract(&app.session).unwrap().bounds);
    app.cam = cam;
    app
}

/// #1454: the same mouse, wheel and modifier input moves the Render preview's camera exactly as
/// it moves the modelling viewport's, under every navigation preset and Orbit around setting.
#[test]
fn render_preview_navigates_like_the_modelling_viewport_in_every_preset() {
    use egui::{Event, Modifiers, PointerButton, pos2, vec2};
    let none = Modifiers::NONE;
    let chords = [none, Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT, Modifiers::SHIFT | Modifiers::ALT];
    let arounds = [crate::prefs::OrbitAround::ViewCentre, crate::prefs::OrbitAround::Cursor, crate::prefs::OrbitAround::Origin];
    for preset in crate::prefs::NAV_PRESETS.iter().map(|p| p.id) {
        for around in arounds {
            let (design_ctx, render_ctx) = (crate::fonts::test_context(), crate::fonts::test_context());
            let mut design = nav_app(false, preset, around);
            let mut render = nav_app(true, preset, around);
            let (mut design_time, mut render_time) = (0.0, 0.0);
            let mut both = |events: Vec<Event>, mods: Modifiers, design: &mut crate::SolveApp, render: &mut crate::SolveApp| {
                nav_frame(design, &design_ctx, false, &mut design_time, mods, events.clone());
                nav_frame(render, &render_ctx, true, &mut render_time, mods, events);
            };
            both(vec![], none, &mut design, &mut render);
            assert_eq!(design.viewport.rect, render.viewport.rect);
            // Presses start on empty space (the Render preview has nothing to pick), and the wheel
            // turns over the model, so zoom to the cursor anchors on its surface.
            let empty = pos2(90.0, 520.0);
            let proj = crate::viewport::projection(&design, design.viewport.rect.unwrap());
            assert!(crate::viewport::pick(&design, &proj, empty).is_empty());
            let model = proj.to_screen(design.cam.target).unwrap();
            assert!(!crate::viewport::pick(&design, &proj, model).is_empty());
            let mut moved = 0;
            for button in [PointerButton::Primary, PointerButton::Middle, PointerButton::Secondary] {
                for mods in chords {
                    let before = design.cam;
                    both(vec![Event::PointerMoved(empty)], mods, &mut design, &mut render);
                    both(vec![Event::PointerButton { pos: empty, button, pressed: true, modifiers: mods }], mods, &mut design, &mut render);
                    let mut at = empty;
                    for _ in 0..5 {
                        at += vec2(14.0, -9.0);
                        both(vec![Event::PointerMoved(at)], mods, &mut design, &mut render);
                    }
                    both(vec![Event::PointerButton { pos: at, button, pressed: false, modifiers: mods }], mods, &mut design, &mut render);
                    for _ in 0..30 {
                        both(vec![], none, &mut design, &mut render);
                    }
                    assert_eq!(design.cam, render.cam, "{preset} {around:?}: {button:?} drag with {mods:?}");
                    moved += usize::from(design.cam != before);
                    // Let any snap settle and start the next gesture from the same view.
                    design.cam = render.cam;
                }
            }
            assert!(moved >= 2, "{preset}: the preset's drags move the camera ({moved})");
            // Wheel over the model, then modifier-only motion (Touchpad: Shift pans, Alt orbits).
            let before = design.cam;
            both(vec![Event::PointerMoved(model)], none, &mut design, &mut render);
            let wheel = Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, 3.0), modifiers: none, phase: egui::TouchPhase::Move };
            both(vec![wheel], none, &mut design, &mut render);
            for _ in 0..30 {
                both(vec![], none, &mut design, &mut render);
            }
            assert_ne!(design.cam, before, "{preset}: the wheel zooms");
            assert_eq!(design.cam, render.cam, "{preset} {around:?}: wheel zoom to the cursor");
            for mods in [Modifiers::SHIFT, Modifiers::ALT] {
                both(vec![Event::PointerMoved(empty)], mods, &mut design, &mut render);
                both(vec![Event::PointerMoved(empty + vec2(25.0, 15.0))], mods, &mut design, &mut render);
                both(vec![Event::PointerMoved(empty)], none, &mut design, &mut render);
                assert_eq!(design.cam, render.cam, "{preset} {around:?}: modifier motion with {mods:?}");
            }
            assert!(render.session.selection.is_empty(), "the Render preview never selects");
        }
    }
}

/// #1454: Fit (F6), a middle double-click, and the axis view's and navigation cube's Home and
/// view controls all work in the Render workspace, framing the product scene.
#[test]
fn render_workspace_view_keys_double_middle_click_and_navigators_work() {
    for style in [crate::prefs::NavigatorStyle::Axes, crate::prefs::NavigatorStyle::Cube] {
        let mut h = Harness::new();
        h.app.preferences.navigator_style = style;
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length":40,"width":30,"height":20,"body_name":"Product"})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        h.frames(3);
        let bounds = solidcraft_engine::product_scene::extract(&h.app.session).unwrap().bounds;
        let mut home = h.app.cam;
        home.set_view(solidcraft_engine::render::StandardView::Iso);
        home.fit(&bounds);
        let off = |h: &mut Harness| {
            h.app.cam.yaw += 0.7;
            h.app.cam.pitch = 0.1;
            h.app.cam.distance *= 3.0;
            h.app.cam.target.x += 25.0;
            h.frames(2);
        };
        // F6 fits, keeping the direction.
        off(&mut h);
        let mut fitted = h.app.cam;
        fitted.fit(&bounds);
        h.step(&json!({"key":"F6"})).unwrap();
        h.settle();
        assert_eq!(h.app.cam, fitted, "{style:?}: F6");
        // A middle double-click goes Home.
        off(&mut h);
        let centre = h.app.viewport.rect.unwrap().center();
        h.step(&json!({"click":[centre.x, centre.y], "double":true, "button":"middle"})).unwrap();
        h.settle();
        assert_eq!(h.app.cam, home, "{style:?}: middle double-click");
        // The navigator's Home control.
        off(&mut h);
        let handle = match style {
            crate::prefs::NavigatorStyle::Axes => "axis_view:home",
            crate::prefs::NavigatorStyle::Cube => "navigation_cube:control:home",
        };
        h.step(&json!({"click":{"handle":handle}})).unwrap();
        h.settle();
        assert!(
            (h.app.cam.yaw - home.yaw).abs() < 1e-6 && (h.app.cam.pitch - home.pitch).abs() < 1e-6,
            "{style:?}: navigator Home turns to the home view"
        );
        // A standard view from the navigator.
        h.step(&json!({"click":{"handle":"axis_view:z"}})).unwrap();
        h.settle();
        assert!(h.app.cam.back().dot(solidcraft_engine::geom::Vec3::Z) > 0.999, "{style:?}: view from +Z, got {:?}", h.app.cam.back());
        assert!(h.app.session.selection.is_empty());
    }
}

/// #1464: a maximised 4K or 5K window is past the preview's pixel budget. The preview draws
/// smaller at the same aspect instead of refusing every frame.
#[test]
fn four_and_five_k_viewports_preview_downscaled_within_the_limits() {
    use solidcraft_engine::render::*;
    for samples in [1, 4, 16] {
        let host = host_executor(16384, true, samples).unwrap();
        let mut s = Session::default();
        s.set_render_viewport(Some(host.clone())).unwrap();
        let request = |width, height| RenderRequest {
            backend: None,
            mode: RenderMode::Preview,
            operation: RenderOperation::Viewport,
            width,
            height,
            format: None,
            features: vec![RenderFeature::Pbr],
            policy: Default::default(),
        };
        if samples <= 4 {
            assert_eq!(preview_size(s.render_viewport(), (1920, 1080)), (1920, 1080), "fits: drawn at full size");
        }
        // 16× MSAA's scratch cost brings even 1080p under the budget.
        let sizes: &[(u64, u64)] = if samples == 16 { &[(1920, 1080), (3840, 2160), (5120, 2880)] } else { &[(3840, 2160), (5120, 2880)] };
        for &full in sizes {
            assert!(resolve_request(&[s.render_backend()], &request(full.0, full.1)).is_err(), "{full:?} at full size is over the limit");
            let (w, h) = preview_size(s.render_viewport(), full);
            assert!(w < full.0 && h < full.1 && w * h <= host.limits.max_pixels, "{full:?} -> {w}x{h}");
            assert!((w as f64 / h as f64 - 16.0 / 9.0).abs() < 0.01, "{w}x{h} keeps the aspect");
            let resolved = resolve_request(&[s.render_backend()], &request(w, h)).unwrap();
            assert_eq!(resolved.executor.as_str(), "gpu-viewport");
            assert!(resolved.estimated_scratch_bytes <= host.limits.max_scratch_bytes);
        }
    }
    assert_eq!(preview_size(None, (5120, 2880)), (5120, 2880), "no executor: resolution reports it");
}

/// Backend failures must remain readable in the fixed-width Render panel, not
/// disappear into an ellipsis before the reason and recovery guidance are shown.
#[test]
fn backend_diagnostics_wrap_without_elision_in_bounded_image_controls() {
    fn collect<'a>(shape: &'a egui::Shape, texts: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::Shape::Text(text) => texts.push(text),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect(shape, texts)),
            _ => {}
        }
    }
    for mode in ["floating", "compact"] {
        for dark in [false, true] {
            let mut h = Harness::new();
            h.app.home.open = false;
            h.app.ui.dark = dark;
            h.call("ui.resize", json!({"width":1400,"height":1800}));
            h.app.run("view.layout", json!({"mode":mode})).unwrap();
            enter(&mut h.app, true).unwrap();
            let reason = "The selected viewport adapter has no compatible Vulkan path device. Images remain available on the CPU; keep the original design and review the driver before retrying.";
            h.app.render_workspace.path_host = PathHost::Failed(reason.into());
            h.frames(3);
            let expected = format!("{}: {reason}", h.app.tr("render.gpu_path_failed"));
            let mut out = h.ctx.run_ui(
                egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 1800.0))), ..Default::default() },
                |ui| {
                    h.app.logic(ui.ctx());
                    ui.set_max_width(285.0);
                    image_controls(&mut h.app, ui);
                },
            );
            out.textures_delta.clear();
            let mut texts = Vec::new();
            out.shapes.iter().for_each(|shape| collect(&shape.shape, &mut texts));
            {
                let message = &expected;
                let text = texts.iter().find(|text| text.galley.job.text == *message).expect("the complete diagnostic is painted");
                assert!(!text.galley.elided, "{mode}, dark={dark}: {message}");
                assert!(text.galley.rows.len() > 1, "long messages must wrap in the narrow panel");
                assert!(text.galley.rect.width() <= 285.0, "the message must not expand the panel");
            }
        }
    }
}

#[test]
fn gpu_material_recipes_use_exact_linear_absorption_without_changing_saved_appearance() {
    use solidcraft_engine::doc::appearance_pack::{GlassBoundary, Recipe, STARTER_PACK};
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Optics"],"color":"#a0c0e0","opacity":0.25})).unwrap();
    h.app.session.mark_saved();
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.render_workspace.view.materials.enabled = true;
    let mut count = 0;
    for preset in STARTER_PACK {
        let Recipe::Glass(recipe) = preset.recipe else { continue };
        let mut binding = materials::Binding::new(target.clone(), &h.app);
        binding.glass_enabled = true;
        binding.glass = materials::Glass::recipe(preset.id).unwrap();
        h.app.render_workspace.view.materials.bindings = vec![binding];
        let mut params = json!({"progressive":true});
        materials::params(&h.app, &mut params).unwrap();
        let wire = &params["raytrace"]["dielectrics"][0];
        assert_eq!(wire["body_name"], "Optics");
        assert_eq!(wire["ior"], recipe.dielectric_ior);
        let [r, g, b]: [f64; 3] = serde_json::from_value(wire["absorption_per_mm"].clone()).unwrap();
        let medium = solidcraft_engine::render::transmission::Transmission::new(solidcraft_engine::geom::Vec3::new(r, g, b), None).unwrap();
        let transmitted = medium.attenuation(recipe.reference_path_mm).unwrap();
        for (actual, expected) in [transmitted.x, transmitted.y, transmitted.z].into_iter().zip(recipe.transmission_linear_rgb) {
            assert!((actual - expected).abs() < 1e-12, "the actual optical consumer must reproduce the recipe's linear transmittance");
        }
        match recipe.boundary {
            GlassBoundary::Closed => {
                assert_eq!(wire["boundary"], "closed_mesh");
                assert_eq!(wire["thickness_mm"], 0.0);
            }
            GlassBoundary::Thin { thickness_mm } => {
                assert_eq!(wire["boundary"], "thin_sheet");
                assert_eq!(wire["thickness_mm"], thickness_mm);
            }
        }
        assert!(wire.get("opacity").is_none());
        count += 1;
    }
    assert_eq!(count, 6);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
    h.app.render_workspace.view.method = ImageMethod::Raster;
    assert!(materials::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.materials.enabled = false;
    let mut raster = json!({});
    materials::params(&h.app, &mut raster).unwrap();
    assert!(raster.get("raytrace").is_none());
}

#[test]
fn gpu_material_identity_requires_explicit_revalidation_and_never_retargets() {
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let mut binding = materials::Binding::new(target, &h.app);
    binding.coat_enabled = true;
    h.app.render_workspace.view.method = ImageMethod::GpuRaytrace;
    h.app.render_workspace.view.materials.enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![binding.clone()];
    materials::params(&h.app, &mut json!({})).unwrap();
    h.app.run("appearance.assign", json!({"bodies":["Optics"],"color":"#123456"})).unwrap();
    assert!(materials::params(&h.app, &mut json!({})).is_err());
    materials::revalidate(&mut h.app, &mut binding).unwrap();
    h.app.render_workspace.view.materials.bindings = vec![binding.clone()];
    materials::params(&h.app, &mut json!({})).unwrap();
    h.app.run("solid.box", json!({"length":3,"width":3,"height":3,"corner":[20,0,0],"body_name":"Other","operation":"new"})).unwrap();
    let scene = h.app.run("render.scene", json!({})).unwrap();
    let target: materials::Target =
        serde_json::from_value(scene["bodies"].as_array().unwrap().iter().find(|body| body["name"] == "Other").unwrap().clone()).unwrap();
    let mut moved = materials::Binding::new(target.clone(), &h.app);
    moved.coat_enabled = true;
    h.app.render_workspace.view.materials.bindings = vec![moved.clone()];
    h.app.run("browser.visibility", json!({"items":["Optics"],"visible":false})).unwrap();
    assert!(materials::params(&h.app, &mut json!({})).is_err());
    assert!(materials::revalidate(&mut h.app, &mut moved).is_err(), "a surviving body with a new ordinal must be selected explicitly");
    assert_eq!(moved.target, target);
    let scene = h.app.run("render.scene", json!({})).unwrap();
    assert_eq!(scene["bodies"][0]["name"], "Other");
    assert_ne!(scene["bodies"][0]["ordinal"], target.ordinal);
    binding.target.name = "Different body".into();
    assert!(materials::revalidate(&mut h.app, &mut binding).is_err());
    assert_eq!(binding.target.name, "Different body");
    reset_document(&mut h.app);
    assert!(h.app.render_workspace.view.materials.bindings.is_empty());
}

#[test]
fn clicked_gpu_glass_recipe_stays_temporary_and_blocks_cpu_rendering() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app.run("solid.box", json!({"length":10,"width":10,"height":3,"body_name":"Optics"})).unwrap();
    h.app.session.mark_saved();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.frames(3);
    let doc = h.app.session.doc.clone();
    let title = h.app.tr("render.material_title").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{title}")}})).unwrap();
    let refresh = h.app.tr("render.material_refresh").to_string();
    h.step(&json!({"widget":refresh})).unwrap();
    let choose = h.app.tr("render.material_choose_body").to_string();
    h.step(&json!({"widget":choose})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:list:render:material-body-0"}})).unwrap();
    let add = h.app.tr("render.material_add").to_string();
    h.step(&json!({"widget":add})).unwrap();
    let glass = h.app.tr("render.material_glass").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{glass}")}})).unwrap();
    h.step(&json!({"click":{"handle":"render:glass:starter.glass.amber-tint"}})).unwrap();
    let binding = &h.app.render_workspace.view.materials.bindings[0];
    assert!(binding.glass_enabled && !binding.coat_enabled && !binding.maps_enabled);
    assert_eq!(binding.glass.recipe, Some("starter.glass.amber-tint"));
    assert_eq!(binding.target.name, "Optics");
    assert!(image_params(&h.app).is_err(), "CPU must not silently ignore active GPU material assignments");
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert!(!h.app.session.is_dirty());
}

#[test]
fn clicked_gpu_material_at_assignment_limit_edits_existing_but_cannot_add_another_body() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1,"body_name":"Optics"})).unwrap();
    h.app.run("solid.pattern.rectangular", json!({"bodies":["Optics"],"dir1":[1,0,0],"count1":129,"spacing1":3})).unwrap();
    let targets: Vec<materials::Target> = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"].clone()).unwrap();
    assert_eq!(targets.len(), 129);
    h.app.session.mark_saved();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.render_workspace.view.materials.bindings = targets.iter().skip(1).cloned().map(|target| materials::Binding::new(target, &h.app)).collect();
    h.frames(3);
    let title = h.app.tr("render.material_title").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{title}")}})).unwrap();
    let refresh = h.app.tr("render.material_refresh").to_string();
    h.step(&json!({"widget":refresh})).unwrap();
    let choose = h.app.tr("render.material_choose_body").to_string();
    h.step(&json!({"widget":choose})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:list:render:material-body-{}", targets[1].ordinal)}})).unwrap();
    let add = h.app.tr("render.material_add").to_string();
    h.step(&json!({"widget":add})).unwrap();
    let include = h.app.tr("render.material_binding_enable").to_string();
    h.step(&json!({"widget":include})).unwrap();
    assert!(!h.app.render_workspace.view.materials.bindings[0].enabled, "the existing assignment remains editable at the limit");
    assert!(h.app.render_workspace.view.materials.bindings.iter().skip(1).all(|binding| binding.enabled));
    assert_eq!(h.app.render_workspace.view.materials.bindings.len(), 128);

    // The first scene body has no assignment: selecting it cannot create a 129th entry.
    h.step(&json!({"widget":refresh})).unwrap();
    h.step(&json!({"widget":choose})).unwrap();
    h.step(&json!({"click":{"handle":format!("dialog:list:render:material-body-{}", targets[0].ordinal)}})).unwrap();
    h.step(&json!({"widget":add})).unwrap();
    assert_eq!(h.app.render_workspace.view.materials.bindings.len(), 128);
    assert!(h.app.render_workspace.view.materials.bindings.iter().all(|binding| binding.target != targets[0]));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    assert!(!h.app.session.is_dirty());
}

#[test]
fn cpu_reflection_controls_preserve_exact_targets_and_engine_bounds() {
    use solidcraft_engine::render::job::ReflectionMode;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Reflector"})).unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::Mirror)).unwrap();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::RoughSpecular)).unwrap();
    let reflections = h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap();
    assert_eq!(reflections.bodies.len(), 1);
    reflections.max_bounces = 8;
    reflections.samples = 64;
    let mut params = json!({});
    cpu::params(&h.app, &mut params).unwrap();
    assert_eq!(params["raytrace"]["reflections"]["bodies"], json!([{"body":"Reflector","mode":"rough_specular"}]));
    assert_eq!(params["raytrace"]["reflections"]["max_bounces"], 8);
    assert_eq!(params["raytrace"]["reflections"]["samples"], 64);
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().max_bounces = 9;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().max_bounces = 8;
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().samples = 65;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.reflections.as_mut().unwrap().samples = 64;
    h.app.session.visibility.hidden_bodies.push("Reflector".into());
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_body_missing")));
    cpu::set_reflection(&mut h.app, "Reflector", None).unwrap();
    assert!(h.app.render_workspace.view.raytrace.reflections.is_none());
    assert!(cpu::set_reflection(&mut h.app, "Missing", Some(ReflectionMode::Mirror)).is_err());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

#[test]
fn cpu_reflection_binding_requires_confirmation_and_never_follows_a_reused_name() {
    use solidcraft_engine::render::job::ReflectionMode;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Reflector"})).unwrap();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::Mirror)).unwrap();
    h.app.run("solid.box", json!({"length":5,"width":5,"height":5,"body_name":"Other"})).unwrap();
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
    assert!(
        cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::RoughSpecular)).unwrap_err().contains(h.app.tr("render.cpu_binding_stale"))
    );
    cpu::revalidate(&mut h.app, "Reflector").unwrap();
    cpu::params(&h.app, &mut json!({})).unwrap();
    h.app.run("body.rename", json!({"body":"Reflector","name":"Original"})).unwrap();
    h.app.run("body.rename", json!({"body":"Other","name":"Reflector"})).unwrap();
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
    assert!(cpu::revalidate(&mut h.app, "Reflector").is_err(), "confirmation cannot move a binding to a different feature");
    let before = json!(h.app.render_workspace.view.raytrace.reflections);
    for mode in [ReflectionMode::Mirror, ReflectionMode::RoughSpecular] {
        assert!(cpu::set_reflection(&mut h.app, "Reflector", Some(mode)).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
        assert_eq!(json!(h.app.render_workspace.view.raytrace.reflections), before, "a stale row's mode cannot rebind its body");
        assert!(cpu::params(&h.app, &mut json!({})).is_err());
    }
    cpu::set_reflection(&mut h.app, "Reflector", None).unwrap();
    cpu::set_reflection(&mut h.app, "Reflector", Some(ReflectionMode::RoughSpecular)).unwrap();
    cpu::params(&h.app, &mut json!({})).unwrap();
    // Isolate document identity: the cloned document has the same bodies, IDs and revision.
    let previous_doc = h.app.session.doc.clone();
    let previous_revision = h.app.session.revision;
    h.app.session = Session::new((*previous_doc).clone());
    h.app.session.revision = previous_revision;
    assert!(!Arc::ptr_eq(&previous_doc, &h.app.session.doc));
    assert!(cpu::params(&h.app, &mut json!({})).unwrap_err().contains(h.app.tr("render.cpu_binding_stale")));
}

#[test]
fn cpu_environment_controls_keep_wire_schemas_separate_without_reading_files() {
    let mut h = Harness::new();
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.hdr = captured_hdr_input("not-opened-by-ui.hdr");
    h.app.render_workspace.view.cpu.rotation_rad = std::f64::consts::FRAC_PI_2;
    h.app.render_workspace.view.cpu.intensity = 2.0;
    h.app.render_workspace.view.cpu.samples = 64;
    let mut hdr = json!({});
    cpu::params(&h.app, &mut hdr).unwrap();
    assert_eq!(
        hdr["environment"],
        json!({"path":"not-opened-by-ui.hdr","expected_sha256":h.app.render_workspace.view.cpu.hdr.resource().unwrap(),"rotation_radians":std::f64::consts::FRAC_PI_2,"intensity":2.0,"samples":64})
    );
    assert!(hdr["raytrace"].get("environment").is_none());
    for gpu_key in ["progressive", "tone_map", "denoise", "lighting", "lens"] {
        assert!(hdr.get(gpu_key).is_none());
    }
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Constant;
    h.app.render_workspace.view.cpu.color = [0.2, 0.4, 0.8];
    let mut constant = json!({});
    cpu::params(&h.app, &mut constant).unwrap();
    assert!(constant.get("environment").is_none());
    assert_eq!(constant["raytrace"]["environment"], json!({"color":[0.2,0.4,0.8],"intensity":2.0}));
    assert!(h.app.render_workspace.view.raytrace.environment.is_none(), "request building must not mutate the editable base");
    h.app.render_workspace.view.cpu.color[0] = f64::NAN;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.samples = 3;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.cpu.samples = 16;
    h.app.render_workspace.view.cpu.hdr = captured_hdr_input("bad\npath.hdr");
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.cpu.environment = cpu::Environment::None;
    let mut none = json!({});
    cpu::params(&h.app, &mut none).unwrap();
    assert!(none.get("environment").is_none());
    assert!(none["raytrace"].get("environment").is_none());
}

#[test]
fn cpu_light_controls_preserve_hidden_spot_fields_and_validate_runtime_bounds() {
    use solidcraft_engine::render::job::{DirectionalLightOptions, PunctualLightOptions, SpotConeOptions};
    let mut h = Harness::new();
    h.app.render_workspace.view.raytrace.lights = Some(vec![DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 2.0 }]);
    h.app.render_workspace.view.raytrace.punctual_lights = Some(vec![PunctualLightOptions {
        position_mm: [0.0, 0.0, 100.0],
        color: [1.0, 0.5, 0.25],
        intensity: 4.0,
        reference_distance_mm: 100.0,
        minimum_distance_mm: 0.01,
        cone: None,
    }]);
    let mut params = json!({});
    cpu::params(&h.app, &mut params).unwrap();
    assert_eq!(params["raytrace"]["lights"][0]["intensity"], 2.0);
    assert_eq!(params["raytrace"]["punctual_lights"][0]["reference_distance_mm"], 100.0);
    assert!(params.get("lighting").is_none(), "CPU must not emit GPU studio lighting");
    h.app.render_workspace.view.raytrace.lights.as_mut().unwrap()[0].direction = [0.0; 3];
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.lights.as_mut().unwrap()[0].direction = [0.0, 0.0, 1.0];
    h.app.render_workspace.view.raytrace.punctual_lights.as_mut().unwrap()[0].minimum_distance_mm = 101.0;
    assert!(cpu::params(&h.app, &mut json!({})).is_err());
    h.app.render_workspace.view.raytrace.punctual_lights.as_mut().unwrap()[0].minimum_distance_mm = 0.01;
    h.app.render_workspace.view.raytrace.punctual_lights.as_mut().unwrap()[0].cone =
        Some(SpotConeOptions { emitted_axis: [0.0, 0.0, -1.0], inner_half_angle_radians: 0.1, outer_half_angle_radians: 0.2 });
    let mut params = json!({});
    cpu::params(&h.app, &mut params).unwrap();
    assert_eq!(
        params["raytrace"]["punctual_lights"][0]["cone"],
        json!(h.app.render_workspace.view.raytrace.punctual_lights.as_ref().unwrap()[0].cone)
    );
}

#[test]
fn delayed_cpu_hdr_picker_cannot_write_into_another_document_or_revision() {
    use crate::picker_test_support::{Slot, open};
    for other_document in [false, true] {
        let mut h = Harness::new();
        let slot: Slot = Default::default();
        open(&mut h.app, &slot);
        h.app.render_workspace.view.cpu.hdr = hdr::Input::FilePending { path: "original.hdr".into() };
        hdr::pick(&mut h.app, hdr::Target::Cpu);
        let ctx = crate::fonts::test_context();
        crate::file_picker::tick(&mut h.app, &ctx);
        assert!(h.app.file_picker_busy());
        if other_document {
            h.app.session = Session::default();
        } else {
            h.app.session.execute("solid.box", &json!({"length":10,"width":20,"height":5})).unwrap();
        }
        *slot.borrow_mut() = Some(Some("late.hdr".into()));
        crate::file_picker::tick(&mut h.app, &ctx);
        assert_eq!(h.app.render_workspace.view.cpu.hdr, hdr::Input::FilePending { path: "original.hdr".into() });
    }
}

#[test]
fn clicked_setup_save_reload_and_switch_keep_gpu_numeric_intent_without_a_host() {
    use solidcraft_engine::doc::render_settings::{GpuJob, RenderJob, SkyEnvironment};
    use solidcraft_engine::render::job::{AreaLight, AreaLights, StudioLighting, Sun, ThinLens, ToneMap};
    use solidcraft_engine::render::lighting::environment::SkyGradient;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.frames(3);
    h.app.session.set_render_job_host(None).unwrap();
    assert!(image_host(&h.app, ImageMethod::GpuRaytrace).is_err());
    h.step(&json!({"click":{"handle":"render:method"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:method-gpu"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.method, ImageMethod::GpuRaytrace);
    h.app.cam.fov = 0.7;
    h.app.ui.perspective = true;
    // The positive UI fixture uses the admitted document boundary. Coordinates
    // beyond it must still refuse before a setup that cannot reopen is created.
    for x in [-2e9, 2e9, 1e9_f64.next_up()] {
        assert!(
            AreaLights::new(&[AreaLight { center: [x, 0.0, 100.0], u: [2.0, 0.0, 0.0], v: [0.0, -3.0, 0.0], radiance: [3.0, 5.0, 7.0] }]).is_err()
        );
    }
    let expected = RenderSetup {
        camera: h.app.cam,
        exposure_ev: 1.25,
        width: 48,
        height: 32,
        job: Some(RenderJob::GpuPath {
            transparent_background: true,
            options: Box::new(GpuJob {
                aging: Default::default(),
                nested_media: true,
                volume: Some(solidcraft_engine::render::job::volume::HomogeneousVolume { extinction_per_mm: 0.0, ..Default::default() }),
                adaptive: Some(solidcraft_engine::render::job::adaptive::AdaptiveSampling {
                    pilot_samples: 16,
                    min_samples: 16,
                    relative_error: 0.1,
                    absolute_error: 0.003,
                    coverage_error: 0.02,
                }),
                transmission_samples: 4,
                finishing: Default::default(),
                backdrop: None,
                linear_exr: false,
                samples: 37,
                max_depth: 11,
                seed: 1977,
                tone_map: ToneMap::AcesFitted,
                denoise: true,
                lens: Some(ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 123.0 }),
                lighting: Some(StudioLighting {
                    environment: [0.25, 0.5, 0.75],
                    sun: Some(Sun { direction: [2.0, 3.0, 4.0], angular_radius_rad: 0.0123456789, radiance: [11.0, 22.0, 33.0] }),
                    area_lights: AreaLights::new(&[AreaLight {
                        center: [1e9, 0.0, 100.0],
                        u: [2.0, 0.0, 0.0],
                        v: [0.0, -3.0, 0.0],
                        radiance: [3.0, 5.0, 7.0],
                    }])
                    .unwrap(),
                }),
                environment: Some(SkyEnvironment::Sky {
                    gradient: SkyGradient { zenith: [0.3, 0.4, 0.5], horizon: [0.6, 0.7, 0.8], ground: [0.1, 0.2, 0.3] },
                    rotation_rad: std::f64::consts::PI,
                    intensity: 1.75,
                }),
                hdr: None,
            }),
        }),
        ..Default::default()
    };
    h.app.run("render.settings.set", json!({"name":"GPU saved","set":expected})).unwrap();
    h.app.render_workspace.view.name = "GPU saved".into();
    setups::load(&mut h.app);
    let completed = h.app.render_workspace.view.completed;
    let result_camera = h.app.render_workspace.view.result_camera;
    h.frames(3);
    // Open the real editors: merely displaying degree controls and RGB fields must be lossless.
    for key in ["render.image_advanced", "render.studio_environment", "render.studio_lighting"] {
        h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr(key))}})).unwrap();
    }
    h.step(&json!({"click":{"handle":format!("dialog:section:{} 1", h.app.tr("render.studio_area"))}})).unwrap();
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    assert!(image_params(&h.app).is_err(), "offline editing must not claim an executable GPU");
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    h.step(&json!({"click":{"handle":"render:setup-name"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"GPU copy"})).unwrap();
    assert_eq!(h.app.render_workspace.view.name, "GPU copy");
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected, "typing Save As must not reset the settings");
    assert!(h.app.render_workspace.view.dirty);
    let undo = h.app.session.undo.len();
    h.step(&json!({"widget":h.app.tr("render.save_setup")})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.render.setups["GPU copy"], expected);
    h.app.render_workspace.view.seed = 8;
    h.app.render_workspace.view.studio.ambient[1] = 0.125;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    h.step(&json!({"widget":h.app.tr("render.reload_settings")})).unwrap();
    if h.app.render_workspace.view.dirty {
        h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    }
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    select_image_method(&mut h.app, ImageMethod::Raster);
    h.app.render_workspace.view.name = "Raster saved".into();
    setups::save(&mut h.app).unwrap();
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:setups"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:setup:GPU copy"}})).unwrap();
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(h.app.render_workspace.view.completed, completed);
    assert_eq!(h.app.render_workspace.view.result_camera, result_camera);
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["GPU copy"], expected, "the boundary setup must reopen from JSON");
    let native = solidcraft_engine::io::write_design_container(&h.app.session.doc).unwrap();
    let reopened = solidcraft_engine::io::read_design(&native).unwrap();
    assert_eq!(reopened.render.setups["GPU copy"], expected, "the native container keeps exact numeric intent");
}

#[test]
fn cpu_setup_reload_preserves_unedited_numeric_options_and_explicit_empty_values() {
    use solidcraft_engine::doc::render_settings::{CpuJob, RenderJob};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2400}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    let options: CpuJob = serde_json::from_value(json!({
        "primary_samples":4,"shadows":false,
        "lights":[{"direction":[2.0,-4.0,8.0],"color":[0.2,0.4,0.6],"intensity":3.0}],
        "punctual_lights":[{"position_mm":[10.0,20.0,30.0],"color":[1.0,0.5,0.25],"intensity":4.0,
            "reference_distance_mm":100.0,"minimum_distance_mm":0.01,
            "cone":{"emitted_axis":[0.0,0.0,-2.0],"inner_half_angle_radians":0.1,"outer_half_angle_radians":0.3}}],
        "area_lights":[{"center_mm":[0.0,0.0,100.0],"edge_u_mm":[20.0,0.0,0.0],"edge_v_mm":[0.0,-30.0,0.0],"color":[0.3,0.5,0.7],"intensity":5.0}],
        "area_samples":16,"seed":9123,"environment":{"color":[0.0,0.0,0.0],"intensity":2.0}
    }))
    .unwrap();
    let mut expected = RenderSetup { camera: h.app.cam, exposure_ev: 1.5, job: Some(RenderJob::CpuRaytrace { options }), ..Default::default() };
    h.app.run("render.settings.set", json!({"name":"CPU numeric","set":expected})).unwrap();
    h.app.render_workspace.view.name = "CPU numeric".into();
    setups::load(&mut h.app);
    h.frames(3);
    for key in ["render.cpu_lights", "render.cpu_directional"] {
        let label = if key == "render.cpu_directional" { format!("{} 1", h.app.tr(key)) } else { h.app.tr(key).to_string() };
        h.step(&json!({"click":{"handle":format!("dialog:section:{label}")}})).unwrap();
    }
    h.frames(3);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    assert_eq!(h.app.render_workspace.view.raytrace.exposure_ev, 0.0, "the UI has one exposure authority");
    h.app.render_workspace.view.draft.background = [2, 4, 8];
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    expected.background = [2, 4, 8];
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(json!(expected)["job"]["options"].get("exposure_ev").is_none());
    for value in [json!({}), json!({"lights":[],"punctual_lights":[],"area_lights":[],"environment":{"color":[0.0,0.0,0.0],"intensity":0.0}})] {
        let options: CpuJob = serde_json::from_value(value).unwrap();
        expected.job = Some(RenderJob::CpuRaytrace { options });
        h.app.run("render.settings.set", json!({"name":"CPU numeric","set":expected})).unwrap();
        setups::load(&mut h.app);
        assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    }
}

#[test]
fn cpu_progressive_controls_resize_only_on_request_and_save_reload_without_a_host() {
    use solidcraft_engine::render::job::CpuProgressiveOptions;
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.raytrace.primary_samples = 16;
    h.app.render_workspace.view.draft.width = 1024;
    h.app.render_workspace.view.draft.height = 1024;
    h.frames(3);
    h.step(&json!({"click":{"handle":"render:cpu-sampling"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-progressive"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.raytrace.primary_samples, 1);
    assert_eq!(h.app.render_workspace.view.raytrace.progressive, Some(CpuProgressiveOptions::default()));
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1024, 1024));
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 64, seed: u32::MAX });
    h.app.render_workspace.view.draft.exposure_ev = 1.25;
    h.app.render_workspace.view.name = "Progressive CPU".into();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    assert!(image_request(&h.app).is_err());
    assert!(setups::save(&mut h.app).is_err());
    h.frames(3);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (1024, 1024));
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    let resize = format!("{} 512 × 512", h.app.tr("render.image_use_largest"));
    assert!(h.widget(&resize).is_some(), "the proposed dimensions must be visible before accepting them");
    h.step(&json!({"widget":resize})).unwrap();
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (512, 512));
    let expected = setups::snapshot(&h.app).unwrap();
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    assert!(image_request(&h.app).unwrap().get("progressive").is_none(), "CPU sampling must not select the GPU executor");
    h.step(&json!({"widget":h.app.tr("render.save_setup")})).unwrap();
    assert_eq!(h.app.session.doc.render.setups["Progressive CPU"], expected);
    h.step(&json!({"click":{"handle":"render:cpu-sampling"}})).unwrap();
    h.step(&json!({"click":{"handle":"render:cpu-fixed"}})).unwrap();
    assert!(h.app.render_workspace.view.raytrace.progressive.is_none());
    assert!(h.app.render_workspace.view.dirty);
    h.step(&json!({"widget":h.app.tr("render.reload_settings")})).unwrap();
    if h.app.render_workspace.view.dirty {
        h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    }
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    h.app.render_workspace.view.name = "Progressive CPU copy".into();
    setups::save(&mut h.app).unwrap();
    let reopened = solidcraft_engine::doc::Document::from_json(&h.app.session.doc.to_json()).unwrap();
    h.app.session = Session::new(reopened);
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert_eq!(h.app.render_workspace.view.raytrace.exposure_ev, 0.0);
    assert_eq!(h.app.render_workspace.view.draft.exposure_ev, 1.25);
}

#[test]
fn cpu_progressive_setup_preserves_hidden_values_empty_lists_and_one_exposure() {
    use solidcraft_engine::doc::render_settings::{CpuJob, RenderJob};
    let mut h = Harness::new();
    h.app.session.set_render_job_host(None).unwrap();
    let options: CpuJob = serde_json::from_value(json!({
        "primary_samples":1,"progressive":{"samples_per_pixel":7,"seed":321},"preset":null,
        "lights":[],"punctual_lights":null,"area_lights":[],"area_samples":16,"seed":9123,
        "shadows":false,"environment":{"color":[0.0,0.0,0.0],"intensity":0.0}
    }))
    .unwrap();
    let expected = RenderSetup {
        camera: h.app.cam,
        width: 32,
        height: 16,
        exposure_ev: -1.5,
        job: Some(RenderJob::CpuRaytrace { options }),
        ..Default::default()
    };
    h.app.run("render.settings.set", json!({"name":"Progressive hidden","set":expected})).unwrap();
    h.app.render_workspace.view.name = "Progressive hidden".into();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    assert_eq!(image_request(&h.app).unwrap()["settings"]["job"]["method"], "gpu_path");
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
    h.app.render_workspace.view.raytrace.progressive.as_mut().unwrap().seed = 654;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    let undo = h.app.session.undo.len();
    setups::save(&mut h.app).unwrap();
    assert_eq!(h.app.session.undo.len(), undo, "a no-op Save must preserve all nonedited intent");
}

#[test]
fn setup_save_refuses_nonportable_state_atomically_and_tracks_invalid_legacy_edits() {
    use solidcraft_engine::render::job::ReflectionMode;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"length":10,"width":20,"height":5,"body_name":"Target"})).unwrap();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    setups::load(&mut h.app);
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    setups::save(&mut h.app).unwrap();
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
    h.app.render_workspace.view.studio.hdr = hdr::Input::FilePending { path: "must-not-be-read.hdr".into() };
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    assert!(setups::save(&mut h.app).unwrap_err().contains(h.app.tr("render.hdr_inspect_required")));
    h.app.render_workspace.view.studio.environment = studio::Environment::None;
    let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
    let binding = materials::Binding::new(target, &h.app);
    h.app.render_workspace.view.materials.bindings.push(binding);
    h.app.render_workspace.view.materials.enabled = false;
    assert!(setups::save(&mut h.app).is_err(), "disabled assignments must not be silently lost either");
    h.app.render_workspace.view.materials.bindings.clear();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    cpu::set_reflection(&mut h.app, "Target", Some(ReflectionMode::Mirror)).unwrap();
    assert!(setups::save(&mut h.app).is_err());
    assert!(h.app.render_workspace.view.raytrace.reflections.is_some());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
    h.app.run("render.settings.set", json!({"name":"Legacy","set":{"job":null,"environment_rotation":0.25,"environment_intensity":2.0}})).unwrap();
    h.app.render_workspace.view.name = "Legacy".into();
    setups::load(&mut h.app);
    assert!(!h.app.render_workspace.view.dirty);
    let doc = h.app.session.doc.clone();
    let undo = h.app.session.undo.len();
    h.app.render_workspace.view.draft.background = [1, 3, 5];
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty, "invalid saved intent still needs complete dirty tracking");
    assert!(setups::save(&mut h.app).unwrap_err().contains(h.app.tr("render.setup_legacy_environment")));
    assert_eq!(h.app.render_workspace.view.draft.environment_rotation, 0.25);
    assert_eq!(h.app.render_workspace.view.draft.environment_intensity, 2.0);
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!(h.app.session.undo.len(), undo);
}

fn original_hdr_bytes(value: u8, declared_primaries: bool) -> Vec<u8> {
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n".to_vec();
    if declared_primaries {
        bytes.extend_from_slice(b"PRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n");
    }
    bytes.extend_from_slice(b"\n-Y 1 +X 2\n");
    bytes.extend_from_slice(&[value, 128, 128, 129, 128, value, 128, 129]);
    bytes
}

/// A verified selector for pure request tests. No file exists or is read by the builder.
fn captured_hdr_input(path: &str) -> hdr::Input {
    let bytes = original_hdr_bytes(128, true);
    hdr::Input::FileCaptured {
        path: path.into(),
        metadata: hdr::Metadata {
            sha256: solidcraft_engine::doc::render_resources::HdrId::of(&bytes),
            width: 2,
            height: 1,
            encoded_bytes: bytes.len() as u64,
            primaries: Some(solidcraft_engine::doc::render_resources::SRGB_D65_PRIMARIES),
        },
    }
}

#[test]
fn hdr_ui_save_embeds_once_without_a_host_and_reload_never_reopens_the_source_path() {
    use solidcraft_engine::doc::render_settings::RenderJob;
    let mut h = Harness::new();
    h.app.run("solid.box", json!({"body_name":"HDR target","length":10,"width":20,"height":5})).unwrap();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.session.set_render_job_host(None).unwrap();
    let path = h.dir.join("original-embedded.hdr");
    let bytes = original_hdr_bytes(128, true);
    std::fs::write(&path, &bytes).unwrap();
    let resource = solidcraft_engine::doc::render_resources::HdrId::of(&bytes);
    let save = h.app.tr("render.save_setup").to_string();
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.name = "HDR GPU".into();
    h.app.render_workspace.view.draft.width = 16;
    h.app.render_workspace.view.draft.height = 16;
    h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
    h.app.render_workspace.view.studio.rotation_rad = 1234.567890123;
    h.app.render_workspace.view.studio.intensity = 1.75;
    h.app.render_workspace.view.samples = 37;
    let before = (h.app.session.revision, h.app.session.undo.len());
    hdr::inspect(&mut h.app, hdr::Target::Gpu, path.to_string_lossy().into_owned()).unwrap();
    assert_eq!((h.app.session.revision, h.app.session.undo.len()), before);
    assert!(h.app.session.doc.render.resources.is_empty());
    let unsaved = image_request(&h.app).unwrap();
    assert_eq!(unsaved["environment"]["expected_sha256"], json!(resource));
    h.frames(3);
    h.step(&json!({"widget":save})).unwrap();
    assert_eq!(h.app.session.undo.len(), before.1 + 1);
    assert_eq!(h.app.render_workspace.view.studio.hdr, hdr::Input::EmbeddedResource { resource });
    assert_eq!(h.app.session.doc.render.resources.len(), 1);
    let gpu = h.app.session.doc.render.setups.get("HDR GPU").unwrap().clone();
    assert!(matches!(&gpu.job, Some(RenderJob::GpuPath { options, .. }) if options.hdr.unwrap().resource == resource && options.samples == 37));
    assert_eq!(image_request(&h.app).unwrap(), json!({"settings":gpu}));
    assert!(!h.app.render_workspace.view.dirty);

    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "HDR CPU".into();
    h.app.render_workspace.view.raytrace.progressive = Some(solidcraft_engine::render::job::CpuProgressiveOptions { samples_per_pixel: 7, seed: 84 });
    h.app.render_workspace.view.draft.exposure_ev = 1.75;
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    h.app.render_workspace.view.cpu.rotation_rad = -1234.567890123;
    h.app.render_workspace.view.cpu.intensity = 2.5;
    h.app.render_workspace.view.cpu.samples = 64;
    hdr::inspect(&mut h.app, hdr::Target::Cpu, path.to_string_lossy().into_owned()).unwrap();
    let undo = h.app.session.undo.len();
    h.frames(2);
    h.step(&json!({"widget":save})).unwrap();
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.render.resources.len(), 1, "both setups share one source payload");
    assert_eq!(h.app.session.doc.render.resources.total_bytes(), bytes.len());
    let cpu = h.app.session.doc.render.setups.get("HDR CPU").unwrap().clone();
    assert!(matches!(&cpu.job, Some(RenderJob::CpuRaytrace { options })
        if options.hdr.unwrap().samples == 64 && options.progressive.unwrap().samples_per_pixel == 7 && options.progressive.unwrap().seed == 84));
    let encoded = h.app.session.doc.to_json();
    assert!(!encoded.contains("original-embedded.hdr"), "the document never stores the source path");
    std::fs::remove_file(&path).unwrap();

    for (name, expected, target) in [("HDR GPU", gpu, hdr::Target::Gpu), ("HDR CPU", cpu, hdr::Target::Cpu)] {
        let loaded = solidcraft_engine::doc::Document::from_json(&encoded).unwrap();
        h.app.session = Session::new(loaded);
        h.app.render_workspace.view.name = name.into();
        setups::load(&mut h.app);
        let camera = expected.camera;
        h.app.cam = camera;
        assert_eq!(hdr::input(&h.app, target), &hdr::Input::EmbeddedResource { resource });
        assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
        assert_eq!(image_request(&h.app).unwrap(), json!({"settings":expected}));
        let before = (h.app.session.revision, h.app.session.undo.len());
        let section = if target == hdr::Target::Cpu { "render.cpu_environment" } else { "render.studio_environment" };
        h.frames(2);
        if target == hdr::Target::Gpu {
            h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr("render.image_advanced"))}})).unwrap();
        }
        h.step(&json!({"click":{"handle":format!("dialog:section:{}", h.app.tr(section))}})).unwrap();
        h.frames(2);
        assert_eq!(setups::snapshot(&h.app).unwrap(), expected, "displaying degrees must preserve saved radians");
        setups::save(&mut h.app).unwrap();
        assert_eq!((h.app.session.revision, h.app.session.undo.len()), before);
        assert_eq!(h.app.cam, camera);
    }
}

#[test]
fn changed_hdr_bytes_reject_save_atomically_and_check_start_keep_the_expected_digest() {
    let mut h = Harness::new();
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "Pinned HDR".into();
    h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
    let path = h.dir.join("changing.hdr");
    std::fs::write(&path, original_hdr_bytes(128, true)).unwrap();
    hdr::inspect(&mut h.app, hdr::Target::Cpu, path.to_string_lossy().into_owned()).unwrap();
    let input = h.app.render_workspace.view.cpu.hdr.clone();
    let request = image_request(&h.app).unwrap();
    let doc = h.app.session.doc.clone();
    let draft = h.app.render_workspace.view.draft.clone();
    let before = (h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len());
    std::fs::write(&path, original_hdr_bytes(64, true)).unwrap();
    assert!(setups::save(&mut h.app).is_err());
    assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
    assert_eq!((h.app.session.revision, h.app.session.undo.len(), h.app.session.redo.len()), before);
    assert_eq!(h.app.render_workspace.view.cpu.hdr, input);
    assert_eq!(h.app.render_workspace.view.draft, draft);
    assert_eq!(image_request(&h.app).unwrap(), request, "both buttons keep the verified digest, never silently accept new bytes");
    assert!(h.app.session.doc.render.resources.is_empty());
    hdr::inspect(&mut h.app, hdr::Target::Cpu, path.to_string_lossy().into_owned()).unwrap();
    assert_ne!(h.app.render_workspace.view.cpu.hdr.resource(), input.resource());
    setups::save(&mut h.app).unwrap();
    assert!(h.app.render_workspace.view.cpu.hdr.embedded());
    assert_eq!(h.app.session.doc.render.resources.len(), 1);
}

#[test]
fn hdr_picker_cancel_and_late_setup_method_or_input_answers_preserve_the_selection() {
    use crate::picker_test_support::{Slot, open};
    for target in [hdr::Target::Cpu, hdr::Target::Gpu] {
        for change in ["cancel", "setup", "method", "input"] {
            let mut h = Harness::new();
            h.app.render_workspace.view.method = if target == hdr::Target::Cpu { ImageMethod::CpuRaytrace } else { ImageMethod::GpuRaytrace };
            h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
            h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
            h.app.render_workspace.view.cpu.hdr = captured_hdr_input("original.hdr");
            h.app.render_workspace.view.studio.hdr = captured_hdr_input("original.hdr");
            let slot: Slot = Default::default();
            open(&mut h.app, &slot);
            hdr::pick(&mut h.app, target);
            let ctx = crate::fonts::test_context();
            crate::file_picker::tick(&mut h.app, &ctx);
            match change {
                "setup" => h.app.render_workspace.view.name = "another setup".into(),
                "method" => h.app.render_workspace.view.method = ImageMethod::Raster,
                "input" => {
                    h.app.render_workspace.view.cpu.hdr = captured_hdr_input("newer.hdr");
                    h.app.render_workspace.view.studio.hdr = captured_hdr_input("newer.hdr");
                }
                _ => {}
            }
            let before = hdr::input(&h.app, target).clone();
            let doc = h.app.session.doc.clone();
            let status = h.app.status.clone();
            *slot.borrow_mut() = Some(if change == "cancel" { None } else { Some("never-read-late.hdr".into()) });
            crate::file_picker::tick(&mut h.app, &ctx);
            assert_eq!(hdr::input(&h.app, target), &before, "{target:?}: {change}");
            assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
            assert_eq!(h.app.status, status, "the stale path must be discarded before trying to inspect it");
        }
    }
}

#[test]
fn embedded_hdr_runtime_requests_preserve_temporary_bindings_without_serializing_them() {
    use solidcraft_engine::render::job::ReflectionMode;
    for target in [hdr::Target::Cpu, hdr::Target::Gpu] {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"body_name":"Target","length":10,"width":20,"height":5})).unwrap();
        enter(&mut h.app, true).unwrap();
        h.app.render_workspace.view.inspector.page = inspector::Page::All;
        select_image_method(&mut h.app, if target == hdr::Target::Cpu { ImageMethod::CpuRaytrace } else { ImageMethod::GpuRaytrace });
        h.app.render_workspace.view.cpu.environment = cpu::Environment::Hdr;
        h.app.render_workspace.view.studio.environment = studio::Environment::Hdr;
        let path = h.dir.join("bindings.hdr");
        std::fs::write(&path, original_hdr_bytes(128, true)).unwrap();
        hdr::inspect(&mut h.app, target, path.to_string_lossy().into_owned()).unwrap();
        setups::save(&mut h.app).unwrap();
        let resource = hdr::input(&h.app, target).resource().unwrap();
        std::fs::remove_file(&path).unwrap();
        if target == hdr::Target::Cpu {
            cpu::set_reflection(&mut h.app, "Target", Some(ReflectionMode::Mirror)).unwrap();
        } else {
            let target: materials::Target = serde_json::from_value(h.app.run("render.scene", json!({})).unwrap()["bodies"][0].clone()).unwrap();
            let binding = materials::Binding::new(target, &h.app);
            h.app.render_workspace.view.materials.bindings.push(binding);
            // Even disabled, this assignment must not be lost on Save. It still
            // selects the runtime path and exercises the embedded HDR selector.
            h.app.render_workspace.view.materials.enabled = false;
        }
        let params = image_request(&h.app).unwrap();
        assert_eq!(params["environment"]["resource"], json!(resource));
        assert!(params["environment"].get("path").is_none());
        assert!(params["environment"].get("expected_sha256").is_none());
        if target == hdr::Target::Cpu {
            assert!(params["environment"].get("kind").is_none());
            assert_eq!(params["raytrace"]["reflections"]["bodies"][0]["body"], "Target");
        } else {
            assert_eq!(params["environment"]["kind"], "embedded_hdr");
        }
        let doc = h.app.session.doc.clone();
        let undo = h.app.session.undo.len();
        assert!(setups::save(&mut h.app).is_err());
        assert!(Arc::ptr_eq(&doc, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), undo);
        assert_eq!(image_request(&h.app).unwrap(), params);
    }
}

#[test]
fn cpu_resource_profile_edits_preserve_resolution_samples_and_saved_intent() {
    use solidcraft_engine::render::job::{CpuBudget, CpuProgressiveOptions};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.app.render_workspace.view.name = "Studio output".into();
    h.app.render_workspace.view.draft.width = 3840;
    h.app.render_workspace.view.draft.height = 2160;
    h.app.render_workspace.view.raytrace.budget = CpuBudget::Studio;
    h.app.render_workspace.view.raytrace.progressive = Some(CpuProgressiveOptions { samples_per_pixel: 128, seed: 82 });
    h.frames(3);
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    h.step(&json!({"click":{"handle":"render:cpu-budget"}})).unwrap();
    h.step(&json!({"click":{"handle":"render.cpu_budget_compact"}})).unwrap();
    h.frames(3);
    assert_eq!((h.app.render_workspace.view.draft.width, h.app.render_workspace.view.draft.height), (3840, 2160));
    assert_eq!(h.app.render_workspace.view.raytrace.progressive.unwrap().samples_per_pixel, 128);
    assert!(setups::snapshot(&h.app).is_err());
    assert!(h.app.render_workspace.view.dirty);
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
}

#[test]
fn large_image_display_uses_captured_profile_and_keeps_export_pixels_unchanged() {
    use solidcraft_engine::render::job::{CpuBudget, ImageRequest, RaytraceOptions};
    let request = ImageRequest {
        integrator: Integrator::Raytrace,
        raytrace: Some(RaytraceOptions { budget: CpuBudget::Studio, ..Default::default() }),
        camera: Camera::default(),
        width: 8,
        height: 8,
        background: [0; 3],
        transparent_background: false,
        environment: None,
        textures: None,
    };
    let limits = captured_image_limits(&request);
    assert_eq!(limits, CpuBudget::Studio.image_limits(false));
    assert_eq!(display_dimensions(7680, 4320, 4096).unwrap(), (4096, 2304));
    assert_eq!(display_dimensions(1920, 1080, 4096).unwrap(), (1920, 1080));
    assert!(display_dimensions(u32::MAX, 1, 0).is_err());
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8200, 2, image::Rgba([42, 88, 123, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let encoded = png.into_inner();
    let before = encoded.clone();
    assert!(decode_image(&encoded, 8200, 2).is_err(), "legacy default remains compact");
    let display = decode_render_image(&encoded, 8200, 2, CpuBudget::Workstation.image_limits(false), 1024).unwrap();
    assert!(display.size[0] <= 1024 && display.size[1] >= 1);
    assert_eq!(encoded, before, "the retained image is not rewritten for its display cache");
    assert!(decode_render_image(&encoded, 8199, 2, CpuBudget::Workstation.image_limits(false), 1024).is_err());
}

#[test]
fn setup_actions_and_return_design_stay_visible_with_long_render_controls() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1280,"height":900}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    h.frames(3);
    for key in ["render.save_setup", "render.reload_settings", "render.return_design"] {
        let label = h.app.tr(key).to_string();
        assert!(h.widget(&label).is_some(), "sticky action {key}");
    }
    h.step(&json!({"widget":h.app.tr("render.return_design")})).unwrap();
    assert!(!active(&h.app));
}

#[test]
fn dirty_indication_preserves_reload_and_resource_picker_hit_targets() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2600}));
    enter(&mut h.app, true).unwrap();
    h.app.render_workspace.view.inspector.page = inspector::Page::All;
    select_image_method(&mut h.app, ImageMethod::CpuRaytrace);
    setups::save(&mut h.app).unwrap();
    setups::load(&mut h.app);
    h.frames(3);
    let reload = h.app.tr("render.reload_settings").to_string();
    let clean_reload = h.widget(&reload).unwrap();
    let clean_picker = crate::scenario::rect_of("render:cpu-budget").unwrap();
    h.app.render_workspace.view.draft.exposure_ev = 1.0;
    setups::refresh_dirty(&mut h.app);
    assert!(h.app.render_workspace.view.dirty);
    h.frames(3);
    assert_eq!(h.widget(&reload).unwrap(), clean_reload, "dirty text must not move Reload");
    assert_eq!(crate::scenario::rect_of("render:cpu-budget").unwrap(), clean_picker, "dirty text must not move the CPU resource picker");
    h.step(&json!({"widget":reload})).unwrap();
    if h.app.render_workspace.view.dirty {
        h.step(&json!({"widget":h.app.tr("render.discard_continue")})).unwrap();
    }
    assert!(!h.app.render_workspace.view.dirty);
    assert_eq!(h.app.render_workspace.view.draft.exposure_ev, 0.0);
}

#[test]
fn product_preview_exact_payload_preserves_mixed_surfaces_and_refuses_overflow() {
    let mut session = Session::default();
    for (name, x) in [("Paint", 0), ("Legacy", 20), ("Glass", 40)] {
        session.execute("solid.box", &json!({"base":[x,0,0],"length":10,"width":10,"height":10,"body_name":name})).unwrap();
    }
    session
        .execute("appearance.assign", &json!({"bodies":["Paint"],"color":"#c07030","pbr":{"metallic":0,"roughness":0.4,"dielectric_ior":1.5}}))
        .unwrap();
    session.execute("appearance.assign", &json!({"bodies":["Glass"],"color":"#336699","opacity":0.4})).unwrap();
    let original = session.doc.to_json();
    let scene = solidcraft_engine::product_scene::extract(&session).unwrap();
    let plan = preview_upload_plan(&scene, MAX_GEOMETRY_BYTES).unwrap();
    let (_, gpu) = gpu_scene(&scene).unwrap();
    assert_eq!((plan.pbr, plan.tris, plan.glass), (gpu.pbr.len(), gpu.tris.len(), gpu.glass.len()));
    assert!(plan.pbr > 0 && plan.tris > 0 && plan.glass > 0);
    assert_eq!(plan.source_triangles, scene.meshes.iter().map(|b| b.mesh.triangles.len()).sum::<usize>());
    assert!(preview_upload_plan(&scene, plan.bytes().unwrap() - 1).is_err());
    assert!(PreviewUploadPlan::default().add(true, false, usize::MAX, MAX_GEOMETRY_BYTES).is_err());
    assert_eq!(session.doc.to_json(), original);
    let mut invalid = scene.clone();
    let source = invalid.meshes[0].mesh.clone();
    Arc::make_mut(&mut invalid.meshes[0].mesh).triangles[0][0] = u32::MAX;
    assert!(gpu_scene(&invalid).is_err());
    assert!(gpu_scene(&scene).is_ok());
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &source));
}

#[cfg(all(not(target_arch = "wasm32"), target_pointer_width = "64"))]
#[test]
fn full_detail_preview_source_can_exceed_legacy_128_mib_without_mesh_reduction() {
    use solidcraft_engine::geom::{Mesh, Vec3};
    use solidcraft_engine::render::{ProductMesh, surface::OpaqueSurface};
    // One immutable original triangle repeated in the index list; this tests
    // payload admission only. Actual SELENE upload is separate native evidence.
    let count = (128 * 1024 * 1024) / (3 * crate::gpu::PBR_SIZE) + 1;
    let mesh = Arc::new(Mesh {
        positions: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        normals: vec![Vec3::Z; 3],
        triangles: vec![[0, 1, 2]; count],
        ..Default::default()
    });
    let body = ProductMesh {
        name: "Original admission fixture".into(),
        mesh: mesh.clone(),
        color: [200, 100, 50],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::new(0.4, 0.2, 0.1), 0.0, 0.4, 1.5).unwrap()),
        transmission: None,
        faces: Vec::new(),
    };
    let scene = ProductScene { meshes: vec![body], bounds: mesh.bounds() };
    assert!(preview_upload_plan(&scene, 128 * 1024 * 1024).is_err());
    let plan = preview_upload_plan(&scene, MAX_GEOMETRY_BYTES).unwrap();
    assert_eq!(plan.source_triangles, count);
    assert_eq!(plan.bytes().unwrap(), count * 3 * crate::gpu::PBR_SIZE);
    assert!(Arc::ptr_eq(&scene.meshes[0].mesh, &mesh));
    let mut maximum = PreviewUploadPlan::default();
    assert!(maximum.add(true, false, MAX_GEOMETRY_BYTES / (3 * crate::gpu::PBR_SIZE) + 1, MAX_GEOMETRY_BYTES).is_err());
}

#[test]
fn refused_product_capture_is_cached_until_identity_changes_or_explicit_retry() {
    let mut h = Harness::new();
    let calls = std::cell::Cell::new(0);
    let signature = (7, 123, 99);
    for _ in 0..8 {
        assert_eq!(
            h.app
                .render_workspace
                .prepare_preview(signature, || {
                    calls.set(calls.get() + 1);
                    Err("original bounded refusal".into())
                })
                .unwrap_err(),
            "original bounded refusal"
        );
    }
    assert_eq!(calls.get(), 1, "same refused source is not rebuilt every frame");
    assert!(h.app.render_workspace.scene.is_none() && h.app.render_workspace.key.is_none());
    assert!(h.app.render_workspace.slot.lock().unwrap().is_none());
    assert!(
        h.app
            .render_workspace
            .prepare_preview((8, 123, 99), || {
                calls.set(calls.get() + 1);
                Err("new revision".into())
            })
            .is_err()
    );
    assert_eq!(calls.get(), 2);
    invalidate_scene(&mut h.app);
    assert!(h.app.render_workspace.scene_signature.is_none() && h.app.render_workspace.scene_error.is_none());
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
    h.app.render_workspace.prepare_preview(signature, || solidcraft_engine::product_scene::extract(&session).map_err(|e| e.to_string())).unwrap();
    assert!(h.app.render_workspace.scene_error.is_none());
    assert!(h.app.render_workspace.preview_upload_bytes.unwrap() > 0);
    assert!(h.app.render_workspace.key.is_some() && h.app.render_workspace.scene.is_some());
}

#[test]
fn product_preview_error_uses_shared_wrapped_surface_and_explicit_retry_target() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1280,"height":800}));
    enter(&mut h.app, true).unwrap();
    h.frames(3);
    let rect = egui::Rect::from_min_size(egui::pos2(260.0, 200.0), egui::vec2(550.0, 360.0));
    let message = "The original captured product geometry exceeds its bounded preview admission. The renderer must preserve all exact body identities and cannot silently simplify or drop geometry to paint this view.";
    let output = h.ctx.run_ui(
        egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))), ..Default::default() },
        |ui| preview_error(&mut h.app, ui, rect, message),
    );
    let _ = output;
    let note = crate::scenario::rect_of("dialog:report:render.preview_failure").unwrap();
    let retry = crate::scenario::rect_of("render:preview-retry").unwrap();
    assert!(rect.contains_rect(note) && rect.contains_rect(retry));
    assert!(note.height() > Tokens::get().font_body, "the detailed failure wraps instead of escaping the viewport");
    assert!(crate::theme::contrast_ratio(Tokens::get().form_error, Tokens::get().form_error_fill) >= 3.0);
}

#[test]
fn finishing_backdrop_and_raw_retention_save_reload_without_a_gpu_or_hidden_transport_edits() {
    use solidcraft_engine::render::job::finishing::{Backdrop, BackdropMode, Finishing};
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":2200}));
    enter(&mut h.app, true).unwrap();
    h.app.session.set_render_job_host(None).unwrap();
    select_image_method(&mut h.app, ImageMethod::GpuRaytrace);
    h.app.render_workspace.view.inspector.page = inspector::Page::Output;
    h.app.render_workspace.view.name = "Linear studio".into();
    h.app.render_workspace.view.studio.finishing =
        Finishing { rgb_gain: [1.3, 0.8, 0.9], saturation: 0.4, contrast: 1.2, vignette: 0.3, ..Default::default() };
    h.app.render_workspace.view.studio.backdrop_enabled = true;
    h.app.render_workspace.view.studio.backdrop = Backdrop { mode: BackdropMode::Radial, center: [0.25, 0.7], ..Default::default() };
    h.app.render_workspace.view.studio.linear_exr = true;
    let expected = setups::snapshot(&h.app).unwrap();
    setups::save(&mut h.app).unwrap();
    h.app.render_workspace.view.studio = studio::View::default();
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
    assert!(!h.app.render_workspace.view.dirty);
    assert!(!studio::linear_export_available(&h.app));
    let request = image_request(&h.app).unwrap();
    assert_eq!(request["settings"]["job"]["options"]["linear_exr"], true);
    assert_eq!(request["settings"]["job"]["options"]["finishing"]["rgb_gain"], json!([1.3, 0.8, 0.9]));
    assert!(image_params(&h.app).is_err(), "offline editing is not a claim of executable GPU capabilities");
    h.app.render_workspace.view.transparent = true;
    assert!(setups::snapshot(&h.app).is_err(), "gradient cannot silently disappear behind alpha mode");
    h.app.render_workspace.view.transparent = false;
    let before = h.app.session.doc.to_json();
    h.frames(3);
    let title = h.app.tr("render.post_title").to_string();
    h.step(&json!({"click":{"handle":format!("dialog:section:{title}")}})).unwrap();
    h.step(&json!({"click":{"handle":"render:post-reset"}})).unwrap();
    assert_eq!(h.app.render_workspace.view.studio.finishing, Finishing::default());
    assert!(h.app.render_workspace.view.studio.linear_exr);
    assert!(h.app.render_workspace.view.studio.backdrop_enabled);
    assert!(h.app.render_workspace.view.dirty);
    assert_eq!(h.app.session.doc.to_json(), before, "reset only edits the render draft");
    setups::load(&mut h.app);
    assert_eq!(setups::snapshot(&h.app).unwrap(), expected);
}

#[test]
fn missing_linear_master_is_never_reconstructed_from_a_finished_png() {
    let mut h = Harness::new();
    h.call("ui.resize", json!({"width":1600,"height":1800}));
    enter(&mut h.app, true).unwrap();
    let id = completed_preview(&mut h, [21, 37, 55]);
    let bytes = h.app.session.render_job_handle(id).unwrap().frame().unwrap().unwrap().png().to_vec();
    h.app.render_workspace.view.completed = Some(id);
    h.app.render_workspace.view.studio.export_exr = true;
    assert!(!studio::linear_export_available(&h.app));
    let temp = std::env::temp_dir().join(format!("solidcraft-missing-linear-{}.exr", std::process::id()));
    assert!(h.app.session.execute("render.export", &json!({"job":id,"path":temp,"format":"exr"})).is_err());
    assert!(!temp.exists());
    assert_eq!(h.app.session.render_job_handle(id).unwrap().frame().unwrap().unwrap().png(), bytes);
}
