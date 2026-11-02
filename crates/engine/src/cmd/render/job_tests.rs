use super::*;
use solidcraft_render::job::{CpuHost, JobId};

fn session() -> (Session, CpuHost) {
    let host = CpuHost::cooperative();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap();
    (s, host)
}

fn request() -> Value {
    json!({"settings":{"width":16,"height":16}})
}
fn start(s: &mut Session) -> JobId {
    serde_json::from_value(s.execute("render.start", &request()).unwrap()["job"].clone()).unwrap()
}
fn finish(host: &CpuHost) {
    for _ in 0..1000 {
        if !host.pump(4096).unwrap() {
            return;
        }
    }
    panic!("cooperative job did not finish");
}

#[test]
fn saved_raster_jobs_and_legacy_requests_share_output_without_runtime_overrides() {
    let (mut s, host) = session();
    s.execute(
        "render.settings.set",
        &json!({"name":"Raster","set":{"width":16,"height":16,"job":{"method":"raster","transparent_background":true}}}),
    )
    .unwrap();
    let named = json!({"setup":"Raster"});
    let checked = s.execute("render.preflight", &named).unwrap();
    let legacy = json!({"settings":{"width":16,"height":16,"job":null},"transparent_background":true});
    assert_eq!(checked, s.execute("render.preflight", &legacy).unwrap());
    let doc = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.evaluations);
    for command in ["render.preflight", "render.start"] {
        for (key, value) in [
            ("progressive", json!(false)),
            ("integrator", json!("raster")),
            ("transparent_background", json!(false)),
            ("raytrace", json!(null)),
            ("environment", json!({"path":"must-not-be-read.hdr"})),
            ("seed", json!(0)),
        ] {
            let mut request = json!({"setup":"Raster","replace":true});
            request[key] = value;
            let error = s.execute(command, &request).unwrap_err().to_string();
            assert!(error.contains("cannot override"), "{error}");
            assert_eq!(host.admitted_bytes(), 0);
            assert!(Arc::ptr_eq(&doc, &s.doc));
            assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations));
        }
    }
    let started = s.execute("render.start", &named).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    finish(&host);
    let status = s.execute("render.status", &json!({"job":id})).unwrap();
    assert_eq!(status["request"], checked["request"]);
    assert_eq!(status["resolved"]["executor"], "cpu-product");
    assert_eq!(status["progress"]["phase"], "completed");
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.evaluations));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn saved_gpu_job_routes_before_dispatch_and_reaches_preflight_and_start_without_hardware() {
    use solidcraft_render::job::{Admission, CancelToken, ImageExecutor, ImageHost, ImageRequest, ImageTask, ImageWork, JobOptions};
    // A protocol executor verifies the captured inputs without probing a physical GPU.
    struct NumericalGpu(&'static str);
    impl ImageExecutor for NumericalGpu {
        fn descriptor(&self) -> solidcraft_render::job::Result<solidcraft_render::ExecutorDescriptor> {
            let mut descriptor = CpuHost::cooperative().descriptor()?;
            descriptor.id = solidcraft_render::ExecutorId::new(self.0).unwrap();
            descriptor.profiles[0].features.extend([solidcraft_render::RenderFeature::Progressive, solidcraft_render::RenderFeature::Hdri]);
            Ok(descriptor)
        }
        fn admit(
            &self,
            scene: &solidcraft_render::ProductScene,
            request: &ImageRequest,
            options: JobOptions,
        ) -> solidcraft_render::job::Result<Admission> {
            options.validate()?;
            solidcraft_render::job::admit(scene, &ImageRequest { environment: None, textures: None, ..request.clone() })
        }
        fn begin(
            &self,
            scene: solidcraft_render::ProductScene,
            request: ImageRequest,
            _: JobOptions,
            cancel: CancelToken,
        ) -> solidcraft_render::job::Result<Box<dyn ImageWork>> {
            Ok(Box::new(ImageTask::new(scene, ImageRequest { environment: None, textures: None, ..request }, cancel)?))
        }
    }
    let mut s = Session::default();
    let camera = solidcraft_render::Camera { fov: 0.8, ..Default::default() };
    s.execute("render.settings.set", &json!({"name":"GPU","set":{
        "width":4,"height":4,"camera":camera,"exposure_ev":1.75,"job":{"method":"gpu_path","transparent_background":true,"options":{
            "samples":7,"max_depth":3,"seed":456,"denoise":true,"tone_map":"aces_fitted",
            "lens":{"aperture_radius_mm":0.5,"focus_distance_mm":20},
            "lighting":{"environment":[0.1,0.3,0.6],"sun":{"direction":[1,-1,-1],"angular_radius_rad":0.05,"radiance":[2,3,4]},"area_lights":[]},
            "environment":{"kind":"sky","rotation_rad":0.75,"intensity":0.5,"gradient":{"zenith":[0.3,0.2,0.1],"horizon":[0.8,0.5,0.3],"ground":[0.1,0.1,0.1]}}
        }}
    }})).unwrap();
    let saved = s.doc.render.setups["GPU"].clone();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.set_render_job_host(Some(CpuHost::cooperative())).unwrap();
    s.register_render_job_host(ImageHost::with_executor(Arc::new(NumericalGpu("another-progressive"))).unwrap()).unwrap();
    let named = json!({"setup":"GPU"});
    for command in ["render.preflight", "render.start"] {
        assert!(s.execute(command, &named).unwrap_err().to_string().contains("gpu-path executor"));
    }
    // Another progressive host cannot replace the saved method, even if registered first.
    let gpu = ImageHost::with_executor(Arc::new(NumericalGpu("gpu-path"))).unwrap();
    s.register_render_job_host(gpu.clone()).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty());
    let checked = s.execute("render.preflight", &named).unwrap();
    let inline = s.execute("render.preflight", &json!({"settings":saved})).unwrap();
    assert_eq!(checked, inline);
    assert_eq!(checked["resolved"]["executor"], "gpu-path");
    assert_eq!(checked["options"]["exposure_ev"], 1.75);
    assert_eq!(checked["options"]["samples"], 7);
    assert_eq!(checked["options"]["lighting"]["environment"], json!([0.1, 0.3, 0.6]));
    assert_eq!(checked["options"]["denoise"], true);
    assert_eq!(checked["environment"]["rotation_rad"], 0.75);
    assert_eq!(gpu.admitted_bytes(), 0);
    let started = s.execute("render.start", &named).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let status = s.render_job_handle(id).unwrap().wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed);
    assert_eq!(status.resolved.executor.as_str(), "gpu-path");
    assert_eq!(json!(status.options), checked["options"]);
    assert_eq!(json!(status.request), checked["request"]);
    assert_eq!(json!(status.environment), checked["environment"]);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty()));

    // The same protocol executor exercises saved HDR lowering after source removal.
    // It verifies admission and immutable inputs, not physical GPU image quality.
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("solidcraft-gpu-saved-hdr-{}-{nonce}.hdr", std::process::id()));
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    std::fs::write(&path, &bytes).unwrap();
    let info = s.execute("render.hdr.inspect", &json!({"path":path})).unwrap();
    let mut hdr_setup = saved.clone();
    let Some(solidcraft_doc::render_settings::RenderJob::GpuPath { options, .. }) = &mut hdr_setup.job else { panic!("GPU fixture") };
    options.environment = None;
    let captured = s
        .execute(
            "render.settings.capture_hdr",
            &json!({"name":"HDR","path":path,"expected_sha256":info["sha256"],
        "rotation_rad":-0.25,"intensity":2.5,"assume_linear_srgb":false,"set":hdr_setup}),
        )
        .unwrap();
    let bad_file =
        json!({"progressive":true,"settings":{"width":4,"height":4},"environment":{"kind":"hdr","path":path,"expected_sha256":"0".repeat(64)}});
    assert!(s.execute("render.preflight", &bad_file).unwrap_err().to_string().contains("changed since selection"));
    std::fs::remove_file(path).unwrap();
    let reopened = solidcraft_io::read_design(&solidcraft_io::write_design_container(&s.doc).unwrap()).unwrap();
    s = Session::new(reopened);
    s.register_render_job_host(gpu.clone()).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.is_dirty());
    let checked = s.execute("render.preflight", &json!({"setup":"HDR"})).unwrap();
    assert_eq!(checked, s.execute("render.preflight", &json!({"settings":captured["setup"]})).unwrap());
    assert_eq!(checked["environment"]["source_sha256"], info["sha256"]);
    assert_eq!(checked["environment"]["kind"], "radiance_hdr");
    assert_eq!(checked["environment"]["color_policy"], "declared_linear_srgb_d65");
    assert_eq!(checked["environment"]["rotation_rad"], -0.25);
    assert_eq!(checked["options"]["exposure_ev"], 1.75);
    assert_eq!(checked["options"]["samples"], 7);
    let runtime = json!({"progressive":true,"settings":{"width":4,"height":4},"environment":{
        "kind":"embedded_hdr","resource":info["sha256"],"rotation_rad":-0.25,"intensity":2.5}});
    assert_eq!(s.execute("render.preflight", &runtime).unwrap()["environment"], checked["environment"]);
    let started = s.execute("render.start", &json!({"setup":"HDR"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let status = s.render_job_handle(id).unwrap().wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed);
    assert_eq!(json!(status.environment), checked["environment"]);
    assert_eq!(json!(status.options), checked["options"]);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.is_dirty()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_job_reopens_and_supplies_exact_options_and_one_exposure_to_check_and_start() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":20,"width":15,"height":10})).unwrap();
    s.execute(
        "render.settings.set",
        &json!({"name":"CPU","set":{"width":16,"height":16,"exposure_ev":1.25,
            "job":{"method":"cpu_raytrace","options":{"primary_samples":4,"shadows":false,"lights":[],
                "punctual_lights":[{"position_mm":[0,0,40],"color":[1,0.5,0.2],"intensity":2,"reference_distance_mm":40,"minimum_distance_mm":0.1,
                    "cone":{"emitted_axis":[0,0,-1],"inner_half_angle_radians":0.2,"outer_half_angle_radians":0.5}}],
                "area_lights":[{"center_mm":[0,0,40],"edge_u_mm":[20,0,0],"edge_v_mm":[0,-10,0],"color":[1,1,1],"intensity":1}],
                "area_samples":4,"seed":85,"environment":{"color":[0.1,0.2,0.3],"intensity":0.5}}}
        }}),
    )
    .unwrap();
    let saved = s.doc.render.setups["CPU"].clone();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty());
    let named = json!({"setup":"CPU"});
    let checked = s.execute("render.preflight", &named).unwrap();
    assert_eq!(checked, s.execute("render.preflight", &json!({"settings":saved})).unwrap());
    assert_eq!(checked["resolved"]["executor"], "cpu-raytrace");
    assert_eq!(checked["request"]["integrator"], "raytrace");
    assert_eq!(checked["request"]["raytrace"]["exposure_ev"], 1.25);
    assert_eq!(checked["request"]["raytrace"]["seed"], 85);
    assert_eq!(checked["request"]["raytrace"]["primary_samples"], 4);
    let started = s.execute("render.start", &named).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let status = s.render_job_handle(id).unwrap().wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
    assert_eq!(json!(status.request), checked["request"]);
    assert_eq!(status.options.exposure_ev, 0.0, "GPU options must not introduce a second CPU exposure");
    assert_eq!(s.doc.render.setups["CPU"], saved);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_hdr_reopens_without_source_and_drives_identical_check_start_and_real_lighting() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":20,"width":20,"height":20})).unwrap();
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("solidcraft-cpu-saved-hdr-{}-{nonce}.hdr", std::process::id()));
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 64, 32, 129]);
    }
    std::fs::write(&path, &bytes).unwrap();
    let info = s.execute("render.hdr.inspect", &json!({"path":path})).unwrap();
    let captured = s
        .execute(
            "render.settings.capture_hdr",
            &json!({"name":"HDR","path":path,"expected_sha256":info["sha256"],
        "rotation_rad":0.75,"intensity":2,"samples":64,"set":{"width":9,"height":9,"exposure_ev":1.25,"background":[13,27,41],
            "job":{"method":"cpu_raytrace","options":{"lights":[],"shadows":false,"primary_samples":4}}}}),
        )
        .unwrap();
    assert!(s.render_job_host().is_none());
    std::fs::remove_file(path).unwrap();
    s = Session::new(solidcraft_io::read_design(&solidcraft_io::write_design_container(&s.doc).unwrap()).unwrap());
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.is_dirty());
    let checked = s.execute("render.preflight", &json!({"setup":"HDR"})).unwrap();
    assert_eq!(checked, s.execute("render.preflight", &json!({"settings":captured["setup"]})).unwrap());
    assert_eq!(checked["request"]["raytrace"]["exposure_ev"], 1.25);
    assert_eq!(checked["environment"]["samples"], 64);
    assert_eq!(checked["environment"]["rotation_radians"], 0.75);
    assert_eq!(checked["environment"]["color_policy"], "explicit_linear_srgb_d65");
    let runtime = json!({"integrator":"raytrace","settings":{"width":9,"height":9,"background":[13,27,41]},
        "raytrace":{"lights":[],"shadows":false,"primary_samples":4,"exposure_ev":1.25},
        "environment":{"resource":info["sha256"],"rotation_radians":0.75,"intensity":2,"samples":64}});
    assert_eq!(checked, s.execute("render.preflight", &runtime).unwrap());
    for environment in [
        json!({"resource":"0".repeat(64)}),
        json!({"resource":info["sha256"],"path":"must-not-open.hdr"}),
        json!({"resource":info["sha256"],"expected_sha256":info["sha256"]}),
    ] {
        let mut invalid = runtime.clone();
        invalid["environment"] = environment;
        assert!(s.execute("render.preflight", &invalid).is_err());
        assert_eq!(host.admitted_bytes(), 0);
    }
    let mut unlit = runtime;
    unlit.as_object_mut().unwrap().remove("environment");
    let started = s.execute("render.start", &unlit).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap().progress.phase, solidcraft_render::job::Phase::Completed);
    let baseline = handle.frame().unwrap().unwrap().png().to_vec();
    let started = s.execute("render.start", &json!({"setup":"HDR"})).unwrap();
    assert_eq!(started["environment"], checked["environment"]);
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
    assert_eq!(json!(status.environment), checked["environment"]);
    assert_eq!(json!(status.request), checked["request"]);
    assert_eq!(status.options.exposure_ev, 0.0);
    assert_ne!(handle.frame().unwrap().unwrap().png(), baseline.as_slice(), "saved bytes must illuminate geometry after reopen");
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.is_dirty()));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn additional_executor_routes_by_capability_and_survives_document_adoption() {
    use solidcraft_render::job::{Admission, CancelToken, ImageExecutor, ImageHost, ImageRequest, ImageTask, ImageWork, JobOptions};
    struct Progressive(&'static str);
    impl ImageExecutor for Progressive {
        fn descriptor(&self) -> solidcraft_render::job::Result<solidcraft_render::ExecutorDescriptor> {
            let mut descriptor = CpuHost::cooperative().descriptor()?;
            descriptor.id = solidcraft_render::ExecutorId::new(self.0).unwrap();
            descriptor.profiles[0].features.push(solidcraft_render::RenderFeature::Progressive);
            descriptor.profiles[0].features.push(solidcraft_render::RenderFeature::Hdri);
            Ok(descriptor)
        }
        fn admit(
            &self,
            scene: &solidcraft_render::ProductScene,
            request: &ImageRequest,
            options: JobOptions,
        ) -> solidcraft_render::job::Result<Admission> {
            options.validate()?;
            // admit_geometry counts source geometry only; this executor renders through the
            // CPU task, which ignores the environment and textures, so budget its pixels and
            // PNG with the CPU estimate for the same frame.
            solidcraft_render::job::admit(scene, &ImageRequest { environment: None, textures: None, ..request.clone() })
        }
        fn begin(
            &self,
            scene: solidcraft_render::ProductScene,
            request: ImageRequest,
            _: JobOptions,
            cancel: CancelToken,
        ) -> solidcraft_render::job::Result<Box<dyn ImageWork>> {
            Ok(Box::new(ImageTask::new(scene, request, cancel)?))
        }
    }
    let (mut session, cpu) = session();
    let original_backend = session.render_backend();
    let reserved = ImageHost::with_executor(Arc::new(Progressive("cpu-raytrace"))).unwrap();
    assert!(session.register_render_job_host(reserved.clone()).unwrap_err().to_string().contains("reserved"));
    assert!(session.set_render_job_host(Some(reserved.clone())).unwrap_err().to_string().contains("reserved"));
    assert_eq!(session.render_backend(), original_backend);
    assert!(Session::default().register_render_job_host(reserved).unwrap_err().to_string().contains("reserved"));
    let additional = ImageHost::with_executor(Arc::new(Progressive("test-progressive"))).unwrap();
    session.register_render_job_host(additional.clone()).unwrap();
    assert!(
        session.register_render_job_host(ImageHost::with_executor(Arc::new(Progressive("test-progressive"))).unwrap()).is_err(),
        "duplicate executor IDs must be rejected"
    );
    let cpu_id = start(&mut session);
    let request = json!({"settings":{"width":16,"height":16},"progressive":true,"samples":3});
    let preflight = session.execute("render.preflight", &request).unwrap();
    assert_eq!(preflight["resolved"]["executor"], "test-progressive");
    assert_eq!(additional.admitted_bytes(), 0);
    let mut studio = request.clone();
    studio["lighting"] = json!({"environment":[1,1,1],"area_lights":[{"center":[0,0,10],"u":[2,0,0],"v":[0,-2,0],"radiance":[4,4,4]}]});
    studio["tone_map"] = json!("aces_fitted");
    let document = session.doc.clone();
    let studio_reply = session.execute("render.preflight", &studio).unwrap();
    assert_eq!(studio_reply["options"]["lighting"]["environment"], json!([1.0, 1.0, 1.0]));
    assert_eq!(studio_reply["options"]["tone_map"], "aces_fitted");
    assert!(Arc::ptr_eq(&document, &session.doc));
    studio["environment"] = json!({"kind":"sky","intensity":0.5,"rotation_rad":0.3});
    let env_reply = session.execute("render.preflight", &studio).unwrap();
    assert_eq!(env_reply["environment"]["kind"], "original_gradient_sky");
    assert_eq!(env_reply["environment"]["width"], 512);
    assert_eq!(env_reply["environment"]["source_sha256"].as_str().unwrap().len(), 64);
    assert!(env_reply["request"].get("environment").is_none());
    assert!(Arc::ptr_eq(&document, &session.doc));
    studio["progressive"] = json!(false);
    assert!(session.execute("render.preflight", &studio).unwrap_err().to_string().contains("require progressive"));
    studio["progressive"] = json!(true);
    studio["lighting"]["area_lights"][0]["u"] = json!([0, 0, 0]);
    assert!(session.execute("render.preflight", &studio).is_err());
    let started = session.execute("render.start", &request).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let status = session.render_job_handle(id).unwrap().wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(status.resolved.executor.as_str(), "test-progressive");
    assert_eq!(status.options.samples, 3);
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed);
    finish(&cpu);
    assert_eq!(session.execute("render.status", &json!({"job":cpu_id})).unwrap()["frame"]["executor"], "cpu-product");
    assert!(session.scratch().execute("render.status", &json!({"job":id})).is_err());
    session.execute("file.new", &json!({})).unwrap();
    assert_eq!(session.execute("render.preflight", &request).unwrap()["resolved"]["executor"], "test-progressive");
    assert!(session.execute("render.status", &json!({"job":id})).is_err());
    for bad in [json!({"samples":0}), json!({"max_depth":17}), json!({"seed":-1}), json!({"samples":4097})] {
        let mut params = request.clone();
        for (key, value) in bad.as_object().unwrap() {
            params[key] = value.clone();
        }
        assert!(session.execute("render.start", &params).is_err());
    }
}

#[test]
fn missing_image_host_is_rejected_before_stale_scene_capture() {
    let mut s = Session::default();
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(command, &params).unwrap();
    }
    assert!(s.model.is_stale());
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &request()).unwrap_err().to_string();
        assert!(error.contains("no image host is installed"), "{error}");
        assert!(s.model.is_stale());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
    }
}

#[test]
fn runtime_commands_preserve_document_history_and_evaluation() {
    let (mut s, host) = session();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    s.execute("render.preflight", &request()).unwrap();
    assert_eq!(host.admitted_bytes(), 0);
    let id = start(&mut s);
    assert_eq!(s.execute("render.status", &json!({"job":id})).unwrap()["progress"]["phase"], "queued");
    finish(&host);
    let status = s.execute("render.status", &json!({"job":id})).unwrap();
    assert_eq!(status["progress"]["phase"], "completed");
    assert_eq!(status["frame"]["job"], json!(id));
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
    assert_eq!(s.execute("render.cancel", &json!({"job":id})).unwrap()["progress"]["phase"], "completed");
}

#[test]
fn omitted_integrator_remains_raster_and_options_do_not_change_viewport_quality() {
    let (mut s, host) = session();
    let implicit = s.execute("render.preflight", &request()).unwrap();
    let explicit = s.execute("render.preflight", &json!({"integrator":"raster","settings":{"width":16,"height":16}})).unwrap();
    assert_eq!(implicit, explicit);
    assert_eq!(implicit["request"]["integrator"], "raster");
    assert_eq!(implicit["resolved"]["executor"], "cpu-product");
    assert!(s.execute("render.preflight", &json!({"raytrace":{"shadows":true}})).is_err());
    assert!(s.execute("render.preflight", &json!({"integrator":"quality"})).is_err());
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(not(feature = "raytrace-cpu"))]
#[test]
fn compiled_out_raytrace_is_rejected_before_scene_capture() {
    let (mut s, host) = session();
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(command, &params).unwrap();
    }
    assert!(s.model.is_stale());
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &json!({"integrator":"raytrace","replace":true})).unwrap_err().to_string();
        assert!(error.contains("not compiled in"), "{error}");
        assert!(s.model.is_stale());
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[test]
fn preflight_and_start_reject_excessive_pixel_work_without_reserving_a_job() {
    let (mut s, host) = session();
    for _ in 0..8 {
        s.execute("solid.box", &json!({"length":20,"width":20,"height":20,"corner":[-10,-10,-10]})).unwrap();
    }
    let mut camera = solidcraft_render::Camera { distance: 1.0, fov: 0.0, ..Default::default() };
    camera.set_view(solidcraft_render::StandardView::Front);
    let params = json!({"settings":{"width":1024,"height":1024,"camera":camera}});
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &params).unwrap_err().to_string();
        assert!(error.contains("128000000 pixel tests"), "{error}");
        assert_eq!(host.admitted_bytes(), 0);
    }
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));

    #[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
    {
        let raytrace_host = CpuHost::for_platform().unwrap();
        s.set_render_job_host(Some(raytrace_host.clone())).unwrap();
        let params = json!({"settings":{"width":1024,"height":1024,"camera":camera},
            "integrator":"raytrace","transparent_background":false});
        let preflight = s.execute("render.preflight", &params).unwrap();
        assert_eq!(preflight["resolved"]["executor"], "cpu-raytrace");
        assert!(preflight["admission"].get("projected_pixel_tests").is_none());
        assert_eq!(raytrace_host.admitted_bytes(), 0);
        let started = s.execute("render.start", &params).unwrap();
        let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
        s.execute("render.cancel", &json!({"job":id})).unwrap();
        let status = s.render_job_handle(id).unwrap().wait_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Cancelled);
        // The worker drops its reservation just after the job turns terminal.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while raytrace_host.admitted_bytes() != 0 {
            assert!(std::time::Instant::now() < deadline, "worker did not release the cancelled job's reservation");
            std::thread::yield_now();
        }
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
        s.set_render_job_host(Some(host.clone())).unwrap();
    }

    let params = json!({"settings":{"width":16,"height":16,"camera":camera}});
    let preflight = s.execute("render.preflight", &params).unwrap();
    assert!(preflight["admission"]["projected_pixel_tests"].as_u64().unwrap() > 0);
    assert_eq!(host.admitted_bytes(), 0);
    let id = s.execute("render.start", &params).unwrap()["job"].clone();
    finish(&host);
    assert_eq!(s.execute("render.status", &json!({"job":id})).unwrap()["progress"]["phase"], "completed");
}

#[test]
fn identities_are_isolated_across_documents_scratch_and_new() {
    let (mut a, host) = session();
    let mut b = Session::default();
    b.set_render_job_host(Some(host.clone())).unwrap();
    let a_id = start(&mut a);
    let b_id = start(&mut b);
    assert_eq!(a_id.serial, b_id.serial);
    assert_ne!(a_id.session, b_id.session);
    assert!(b.execute("render.cancel", &json!({"job":a_id})).is_err());
    assert!(a.scratch().execute("render.status", &json!({"job":a_id})).is_err());
    a.execute("file.new", &json!({})).unwrap();
    assert!(a.render_job_host().unwrap().same_host(&host));
    assert!(a.execute("render.status", &json!({"job":a_id})).is_err());
    finish(&host);
    assert_eq!(b.execute("render.status", &json!({"job":b_id})).unwrap()["progress"]["phase"], "completed");
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_progressive_check_start_and_export_preserve_the_exact_final() {
    use solidcraft_render::job::Phase;
    let host = CpuHost::for_platform().unwrap();
    let mut s = Session::default();
    s.execute(
        "render.settings.set",
        &json!({"name":"Film","set":{"width":16,"height":16,"exposure_ev":1.25,
            "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":81},"lights":[]}}
        }}),
    )
    .unwrap();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.set_render_job_host(Some(host)).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    let params = json!({"setup":"Film"});
    let checked = s.execute("render.preflight", &params).unwrap();
    assert_eq!(checked["resolved"]["executor"], "cpu-raytrace");
    assert_eq!(checked["request"]["raytrace"]["progressive"], json!({"samples_per_pixel":3,"seed":81}));
    assert_eq!(checked["request"]["raytrace"]["exposure_ev"], 1.25);
    let started = s.execute("render.start", &params).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    assert_eq!(json!(status.request), checked["request"]);
    assert!(status.preview.is_none());
    assert!(handle.preview().unwrap().is_none());
    let frame = handle.frame().unwrap().unwrap();
    let path = std::env::temp_dir().join(format!("solidcraft-film-final-{}-{}.png", std::process::id(), id.session));
    s.execute("render.export", &json!({"job":id,"path":path})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), frame.png());
    assert_eq!(frame.info.png_sha256.len(), 64);
    std::fs::remove_file(path).unwrap();
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn progressive_limits_and_incompatibilities_fail_before_hdr_or_scene_capture() {
    let host = CpuHost::for_platform().unwrap();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for (width, height, samples, primary) in [(2048, 1024, 1, 1), (1024, 1024, 17, 1), (16, 16, 65, 1), (16, 16, 4, 4)] {
        let params = json!({"integrator":"raytrace","settings":{"width":width,"height":height},
            "raytrace":{"primary_samples":primary,"progressive":{"samples_per_pixel":samples}},
            "environment":{"path":"deliberately-absent-film-admission.hdr"}});
        for command in ["render.preflight", "render.start"] {
            let message = s.execute(command, &params).unwrap_err().to_string();
            let expected = if primary != 1 { "CPU progressive" } else { "CPU Compact" };
            assert!(message.contains(expected), "{message}");
            assert!(!message.contains("deliberately-absent"), "HDR was read before numerical admission");
            assert_eq!(host.admitted_bytes(), 0);
        }
    }
    let wrong_method = json!({"progressive":true,"raytrace":{"progressive":{"samples_per_pixel":4}}});
    assert!(s.execute("render.preflight", &wrong_method).is_err(), "CPU sampling must not fall back to the GPU method");
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn a_cancelled_progressive_preview_cannot_replace_or_export_over_the_last_completed_frame() {
    use solidcraft_render::job::Phase;
    use std::time::{Duration, Instant};
    let mut s = Session::default();
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    let good = start(&mut s);
    assert_eq!(s.render_job_handle(good).unwrap().wait_timeout(Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
    let final_frame = s.render_jobs.frame(good).unwrap();
    let started = s
        .execute(
            "render.start",
            &json!({"integrator":"raytrace","settings":{"width":256,"height":256},
        "raytrace":{"progressive":{"samples_per_pixel":64,"seed":27},"lights":[]}}),
        )
        .unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    assert!(Session::default().render_job_handle(id).is_err());
    let watch = Instant::now();
    let preview = loop {
        if let Some(preview) = handle.preview().unwrap() {
            break preview;
        }
        let status = handle.status().unwrap();
        assert!(!status.progress.phase.terminal(), "progressive worker terminated before a preview could be observed: {:?}", status.error);
        assert!(watch.elapsed() < Duration::from_secs(60), "worker did not publish a bounded checkpoint");
        std::thread::yield_now();
    };
    assert_eq!(preview.info.frame.job, id);
    assert!(handle.frame().unwrap().is_none());
    s.execute("render.cancel", &json!({"job":id})).unwrap();
    assert_eq!(handle.wait_timeout(Duration::from_secs(60)).unwrap().progress.phase, Phase::Cancelled);
    assert!(handle.preview().unwrap().is_none());
    assert!(handle.frame().unwrap().is_none());
    assert!(s.render_jobs.frame(id).is_err());
    let path = std::env::temp_dir().join(format!("solidcraft-film-cancel-{}-{}.png", std::process::id(), id.session));
    s.execute("render.export", &json!({"job":good,"path":path})).unwrap();
    assert!(s.execute("render.export", &json!({"job":id,"path":path,"overwrite":true})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), final_frame.png());
    assert_eq!(s.render_jobs.frame(good).unwrap().info.png_sha256, final_frame.info.png_sha256);
    assert_eq!(preview.info.frame.png_sha256.len(), 64);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn queue_cancellation_and_replacement_keep_the_last_successful_image() {
    let (mut s, host) = session();
    let good = start(&mut s);
    finish(&host);
    s.execute("render.status", &json!({"job":good})).unwrap();
    let a = start(&mut s);
    host.pump(1).unwrap();
    start(&mut s);
    assert!(s.execute("render.start", &request()).is_err());
    s.execute("render.cancel", &json!({"job":a})).unwrap();
    let replacement = s.execute("render.start", &json!({"settings":{"width":16,"height":16},"replace":true})).unwrap();
    s.execute("render.cancel", &json!({"job":replacement["job"]})).unwrap();
    finish(&host);
    assert!(s.render_jobs.frame(good).is_ok());
    assert_eq!(s.execute("render.status", &json!({"job":a})).unwrap()["progress"]["phase"], "cancelled");
}

#[test]
fn unsupported_intent_is_rejected_and_export_preserves_existing_files() {
    let (mut s, host) = session();
    for params in [
        json!({"settings":{"mode":"final"}}),
        json!({"settings":{"format":"exr"}}),
        json!({"settings":{"backend":"missing"}}),
        json!({"settings":{"exposure_ev":1},"integrator":"raytrace"}),
        json!({"unknown":true}),
    ] {
        assert!(s.execute("render.preflight", &params).is_err());
        assert_eq!(host.admitted_bytes(), 0);
    }
    let id = start(&mut s);
    let path = std::env::temp_dir().join(format!("solidcraft-job-{}-{}.png", std::process::id(), id.session));
    assert!(s.execute("render.export", &json!({"job":id,"path":path})).is_err());
    finish(&host);
    let missing = path.with_file_name("solidcraft-render-missing-directory").join("image.png");
    assert!(s.execute("render.export", &json!({"job":id,"path":missing})).is_err());
    assert!(s.render_jobs.frame(id).is_ok());
    s.execute("render.export", &json!({"job":id,"path":path})).unwrap();
    let png = std::fs::read(&path).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    assert!(s.execute("render.export", &json!({"job":id,"path":path})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), png);
    s.execute("render.export", &json!({"job":id,"path":path,"overwrite":true})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), png);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn open_close_and_host_replacement_cancel_work_and_reject_old_ids() {
    let (mut s, host) = session();
    let path = std::env::temp_dir().join(format!("solidcraft-job-open-{}.solidcraft", std::process::id()));
    s.execute("file.save", &json!({"path":path})).unwrap();
    let id = start(&mut s);
    let handle = s.render_job_handle(id).unwrap();
    s.execute("doc.open", &json!({"path":path})).unwrap();
    assert_eq!(handle.status().unwrap().progress.phase, solidcraft_render::job::Phase::Cancelled);
    assert!(s.execute("render.status", &json!({"job":id})).is_err());
    assert!(s.render_job_host().unwrap().same_host(&host));
    let id = start(&mut s);
    let handle = s.render_job_handle(id).unwrap();
    s.replace_design(Session::default());
    assert_eq!(handle.status().unwrap().progress.phase, solidcraft_render::job::Phase::Cancelled);
    assert!(s.execute("render.status", &json!({"job":id})).is_err());
    assert!(s.render_job_host().unwrap().same_host(&host));
    let id = start(&mut s);
    let handle = s.render_job_handle(id).unwrap();
    let replacement = CpuHost::cooperative();
    s.set_render_job_host(Some(replacement.clone())).unwrap();
    assert_eq!(handle.status().unwrap().progress.phase, solidcraft_render::job::Phase::Cancelled);
    assert!(s.execute("render.status", &json!({"job":id})).is_err());
    let id = start(&mut s);
    let handle = s.render_job_handle(id).unwrap();
    drop(s);
    assert_eq!(handle.status().unwrap().progress.phase, solidcraft_render::job::Phase::Cancelled);
    finish(&host);
    finish(&replacement);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn captured_image_is_immutable_across_edit_and_undo_and_stale_preflight_fails() {
    let (mut s, host) = session();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":10})).unwrap();
    let revision = s.revision;
    let id = start(&mut s);
    s.execute("solid.box", &json!({"length":3,"width":4,"height":5})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    finish(&host);
    let status = s.execute("render.status", &json!({"job":id})).unwrap();
    assert_eq!(status["scene_generation"], revision);
    assert_eq!(status["progress"]["phase"], "completed");
    assert_ne!(s.revision, revision);
    // Reuse the real live-gesture regression rather than manufacturing model state.
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(command, &params).unwrap();
    }
    assert!(s.model.is_stale());
    let evaluations = s.evaluations;
    assert!(s.execute("render.preflight", &request()).is_err());
    assert!(s.execute("render.start", &request()).is_err());
    assert!(s.model.is_stale());
    assert_eq!(s.evaluations, evaluations);
}

#[test]
fn native_start_and_cancel_measure_cached_capture_without_timing_assertions() {
    use std::time::{Duration, Instant};
    let mut s = Session::default();
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    s.execute("solid.box", &json!({"length":40,"width":30,"height":20})).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let before = Instant::now();
    let reply = s.execute("render.start", &json!({})).unwrap();
    let start_time = before.elapsed();
    let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
    let before = Instant::now();
    s.execute("render.cancel", &json!({"job":id})).unwrap();
    let cancel_request_time = before.elapsed();
    let status = s.render_job_handle(id).unwrap().wait_timeout(Duration::from_secs(60)).unwrap();
    let cancellation_time = before.elapsed();
    assert!(matches!(status.progress.phase, solidcraft_render::job::Phase::Cancelled | solidcraft_render::job::Phase::Completed));
    if status.progress.phase == solidcraft_render::job::Phase::Cancelled {
        assert!(status.frame.is_none());
    }
    eprintln!(
        "cached one-box 1024x768: start={start_time:?}; cancel request={cancel_request_time:?}; cancellation acknowledged={cancellation_time:?}"
    );
    let reply = s.execute("render.start", &json!({})).unwrap();
    let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let watch = Instant::now();
    let observed = loop {
        let phase = handle.status().unwrap().progress.phase;
        if phase == solidcraft_render::job::Phase::Running || phase.terminal() {
            break phase;
        }
        assert!(watch.elapsed() < Duration::from_secs(60), "native worker did not advance");
        std::thread::yield_now();
    };
    let before = Instant::now();
    s.execute("render.cancel", &json!({"job":id})).unwrap();
    let request_time = before.elapsed();
    let status = handle.wait_timeout(Duration::from_secs(60)).unwrap();
    eprintln!(
        "native cancellation observed phase={observed:?}: request={request_time:?}; acknowledged={:?}; terminal={:?}",
        before.elapsed(),
        status.progress.phase
    );
    assert!(status.progress.phase.terminal());
}
#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn native_raytrace_preflight_start_and_frame_preserve_document_and_roundtrip_request() {
    let mut s = Session::default();
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    s.execute("solid.box", &json!({"length":20,"width":15,"height":10})).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    let params = json!({"integrator":"raytrace","settings":{"width":32,"height":32},"raytrace":{"preset":"neutral","shadows":true}});
    let preflight = s.execute("render.preflight", &params).unwrap();
    let request: solidcraft_render::job::ImageRequest = serde_json::from_value(preflight["request"].clone()).unwrap();
    assert_eq!(request.integrator, solidcraft_render::job::Integrator::Raytrace);
    assert_eq!(preflight["resolved"]["executor"], "cpu-raytrace");
    let started = s.execute("render.start", &params).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed);
    let frame = handle.frame().unwrap().unwrap();
    assert_eq!(frame.info.job, id);
    assert_eq!(frame.info.executor.as_str(), "cpu-raytrace");
    assert_eq!((frame.info.width, frame.info.height), (32, 32));
    assert!(frame.png().starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(frame.info.png_bytes, frame.png().len() as u64);
    assert_eq!(frame.info.png_sha256.len(), 64);
    assert_eq!(status.frame.unwrap().png_sha256, frame.info.png_sha256);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn captured_hdr_input_lights_geometry_after_source_removal_and_preserves_history() {
    let host = CpuHost::for_platform().unwrap();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("solidcraft-hdri-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let source = dir.join("original.hdr");
    s.execute("solid.box", &json!({"length":20,"width":20,"height":20})).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let bytes = |primaries: &str| {
        let mut b = format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n{primaries}\n-Y 2 +X 4\n").into_bytes();
        for _ in 0..8 {
            b.extend_from_slice(&[128, 64, 32, 129]);
        }
        b
    };
    std::fs::write(&source, bytes("")).unwrap();
    let params = json!({"integrator":"raytrace","raytrace":{"lights":[]},"settings":{"width":9,"height":9,"background":[13,27,41]},"environment":{"path":source.to_str().unwrap(),"samples":64}});
    let doc = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), s.is_dirty());
    assert!(s.execute("render.preflight", &params).unwrap_err().to_string().contains("primaries"));
    assert_eq!(host.admitted_bytes(), 0);
    std::fs::write(&source, bytes("PRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n")).unwrap();
    let preflight = s.execute("render.preflight", &params).unwrap();
    assert_eq!(preflight["environment"]["color_policy"], "explicit_linear_srgb_d65");
    assert_eq!(preflight["environment"]["header"]["width"], 4);
    assert!(preflight["resolved"]["profile"]["features"].as_array().unwrap().contains(&json!("hdri")));
    assert_eq!(preflight["environment"]["source_sha256"].as_array().unwrap().len(), 32);
    assert!(!preflight["environment"].to_string().contains(dir.to_str().unwrap()));
    assert_eq!(host.admitted_bytes(), 0);
    let mut unlit = params.clone();
    unlit.as_object_mut().unwrap().remove("environment");
    let base = s.execute("render.start", &unlit).unwrap();
    let base_id: JobId = serde_json::from_value(base["job"].clone()).unwrap();
    let base_handle = s.render_jobs.handle(base_id).unwrap();
    base_handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    let baseline = base_handle.frame().unwrap().unwrap().png().to_vec();
    let reply = s.execute("render.start", &params).unwrap();
    assert_eq!(reply["environment"], preflight["environment"]);
    let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
    // Captured bytes remain sufficient after the source disappears.
    std::fs::remove_file(source).unwrap();
    let handle = s.render_jobs.handle(id).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, solidcraft_render::job::Phase::Completed);
    assert_ne!(handle.frame().unwrap().unwrap().png(), baseline.as_slice(), "captured HDR must illuminate actual geometry");
    assert_eq!(serde_json::to_value(handle.status().unwrap()).unwrap()["environment"], preflight["environment"]);
    let output = dir.join("background.png");
    s.execute("render.export", &json!({"job":id,"path":output.to_str().unwrap()})).unwrap();
    assert!(std::fs::metadata(output).unwrap().len() > 0);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), s.is_dirty()));
    std::fs::remove_dir_all(dir).unwrap();
}

// Spatial-AA transport regressions: WRITTEN NOT RUN in this source epoch.
#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn native_spatial_aa_preflight_and_host_forwarding_change_actual_edge_png() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("solid.box", &json!({"length":20,"width":15,"height":10})).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let scene = crate::product_scene::extract(&s).unwrap();
    let center = scene.bounds.center();
    let mut camera = solidcraft_render::Camera { distance: 2.0, fov: 0.0, ..Default::default() };
    camera.set_view(solidcraft_render::StandardView::Top);
    camera.target = solidcraft_geom::Vec3::new(scene.bounds.min.x, center.y, center.z);
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    let mut legacy: Option<Vec<u8>> = None;
    let mut spatial: Option<Vec<u8>> = None;
    for samples in [1, 4, 16] {
        // Darkness makes covered geometry black; the tiny orthographic pixel
        // straddles the box's left edge against an exact white background.
        let params = json!({"integrator":"raytrace", "settings":{"width":1,"height":1,"camera":camera,"background":[255,255,255]},
            "raytrace":{"primary_samples":samples,"lights":[],"shadows":false}});
        let admitted = host.admitted_bytes();
        let checked = s.execute("render.preflight", &params).unwrap();
        let request: solidcraft_render::job::ImageRequest = serde_json::from_value(checked["request"].clone()).unwrap();
        assert_eq!(request.raytrace.as_ref().unwrap().primary_samples, samples);
        assert_eq!(checked["resolved"]["executor"], "cpu-raytrace");
        assert_eq!(host.admitted_bytes(), admitted, "preflight reserved host memory");
        let started = s.execute("render.start", &params).unwrap();
        let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
        let handle = s.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed);
        assert_eq!(status.request.raytrace.as_ref().unwrap().primary_samples, samples);
        let frame = handle.frame().unwrap().unwrap();
        assert_eq!(frame.info.executor.as_str(), "cpu-raytrace");
        assert_eq!((frame.info.width, frame.info.height), (1, 1));
        let png = frame.png().to_vec();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        if samples == 1 {
            legacy = Some(png);
        } else {
            assert_ne!(png.as_slice(), legacy.as_ref().unwrap().as_slice(), "AA was not forwarded into actual work");
            if let Some(previous) = &spatial {
                assert_eq!(&png, previous, "half coverage differs between4 and16");
            }
            spatial = Some(png);
        }
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn spatial_aa_invalid_or_compiled_out_requests_refuse_before_stale_capture_and_reservation() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    for (command, params) in [
        ("sketch.create", json!({"plane":"XY"})),
        ("sketch.rectangle.two_point", json!({"p0":[5,5],"p1":[25,20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance":5})),
        ("sketch.edit", json!({"sketch":"Sketch1"})),
        ("sketch.drag", json!({"entity":"l1.start","from":[5,5],"to":[4,5],"live":true})),
    ] {
        s.execute(command, &params).unwrap();
    }
    assert!(s.model.is_stale());
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    #[cfg(feature = "raytrace-cpu")]
    let counts: &[u8] = &[0, 2, 8, 17, 255];
    #[cfg(not(feature = "raytrace-cpu"))]
    let counts: &[u8] = &[0, 2, 4, 8, 16, 17, 255];
    for samples in counts {
        for command in ["render.preflight", "render.start"] {
            let params = json!({"integrator":"raytrace","replace":true,"settings":{"width":1,"height":1},"raytrace":{"primary_samples":samples}});
            let error = s.execute(command, &params).unwrap_err().to_string();
            #[cfg(feature = "raytrace-cpu")]
            assert!(error.contains("primary samples"), "{error}");
            #[cfg(not(feature = "raytrace-cpu"))]
            assert!(error.contains("not compiled in"), "{error}");
            assert!(s.model.is_stale());
            assert!(Arc::ptr_eq(&doc, &s.doc));
            assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
            assert_eq!(host.admitted_bytes(), 0);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn completed_png_content_identity_matches_status_and_exact_retained_export() {
    let (mut s, host) = session();
    s.execute("solid.box", &json!({"body_name":"Digest specimen","length":10,"width":20,"height":5})).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let physical = s.execute("component.properties", &json!({})).unwrap();
    let doc = s.doc.clone();
    let history = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    // The ID is the actual command reply, not a fabricated counter.
    let id = start(&mut s);
    finish(&host);
    let before_export = s.execute("render.status", &json!({"job":id})).unwrap();
    assert_eq!(before_export["progress"]["phase"], "completed");
    let metadata = before_export["frame"].clone();
    let frame = s.render_jobs.frame(id).unwrap();
    assert_eq!(metadata["job"], json!(id));
    assert_eq!(metadata["scene_generation"], history.0);
    assert_eq!(metadata["png_bytes"], frame.png().len() as u64);
    let digest = metadata["png_sha256"].as_str().unwrap();
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)));
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("solidcraft-frame-digest-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("retained.png");
    assert!(!path.exists());
    let exported = s.execute("render.export", &json!({"job":id,"path":path})).unwrap();
    assert_eq!(exported["frame"], metadata);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes, frame.png(), "export must write these exact already-hashed retained bytes");
    assert_eq!(bytes.len() as u64, metadata["png_bytes"].as_u64().unwrap());
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 16);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 16);
    assert_eq!(s.execute("render.status", &json!({"job":id})).unwrap(), before_export);
    assert!(s.execute("render.export", &json!({"job":id,"path":path})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), physical);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()), history);
    // A later completed image has a different identity. An evicted old ID must
    // never export the newer PNG, even while this test retains its old Arc.
    let replacement: JobId =
        serde_json::from_value(s.execute("render.start", &json!({"settings":{"width":8,"height":8}})).unwrap()["job"].clone()).unwrap();
    finish(&host);
    let newer = s.execute("render.status", &json!({"job":replacement})).unwrap();
    assert_ne!(newer["frame"]["png_sha256"], metadata["png_sha256"]);
    let still_retained = dir.join("still-retained.png");
    s.execute("render.export", &json!({"job":id,"path":still_retained})).unwrap();
    assert_eq!(std::fs::read(still_retained).unwrap(), bytes);
    for _ in 0..7 {
        let next = start(&mut s);
        finish(&host);
        s.execute("render.status", &json!({"job":next})).unwrap();
    }
    let evicted = dir.join("evicted.png");
    assert!(s.execute("render.export", &json!({"job":id,"path":evicted})).is_err());
    assert!(!evicted.exists());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn progressive_refuses_cpu_runtime_options_before_capture_or_hdr_read() {
    let mut s = Session::default();
    let document = s.doc.clone();
    let state = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for cpu in [
        json!({"integrator":"raytrace"}),
        json!({"raytrace":{"primary_samples":4}}),
        json!({"environment":{"path":"must-not-be-read.hdr"}}),
        json!({"led_samples":4}),
    ] {
        let mut input = cpu;
        input["progressive"] = json!(true);
        for command in ["render.preflight", "render.start"] {
            let error = s.execute(command, &input).unwrap_err().to_string();
            assert!(error.contains("progressive rendering does not accept CPU"), "{error}");
        }
    }
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(state, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[test]
fn runtime_texture_controls_require_progressive_and_leave_document_unchanged() {
    let (mut s, _) = session();
    let doc = s.doc.clone();
    let input = json!({"raytrace":{"textures":[{"body_index":0,"body_name":"No body","frame":{"origin":[0,0,0],"u":[1,0,0],"v":[0,1,0]},"normal_opengl":"not-opened.png"}]}});
    let error = s.execute("render.preflight", &input).unwrap_err().to_string();
    assert!(error.contains("require progressive"), "{error}");
    assert!(Arc::ptr_eq(&doc, &s.doc));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn dielectric_preflight_and_start_resolve_refraction_without_broadening_boundaries() {
    let host = CpuHost::for_platform().unwrap();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("solid.box", &json!({"length":20,"width":20,"height":2,"body_name":"Glass"})).unwrap();
    // The default view sends the single pixel's ray exactly through a box edge, which the optics reject
    // as an ambiguous interface normal. Whether the job then ends Failed or Cancelled would depend on
    // whether the cancel below beats the render thread, so view the glass off-axis.
    let camera = solidcraft_render::Camera { yaw: 0.3, pitch: 0.4, ..Default::default() };
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    let params = json!({"integrator":"raytrace","settings":{"width":1,"height":1,"camera":camera},"raytrace":{"dielectrics":[{
        "body_index":0,"body_name":"Glass","boundary":"closed_box","ior":1.5,"absorption_per_mm":[0,0,0],"thickness_mm":0
    }]}});
    for command in ["render.preflight", "render.start"] {
        let reply = s.execute(command, &params).unwrap();
        if command == "render.preflight" {
            assert_eq!(reply["refraction"], true);
            assert_eq!(reply["transparency"], "physical_transmission");
            assert_eq!(reply["dielectric_boundaries"], json!(["thin_sheet", "closed_box"]));
        }
        assert_eq!(reply["resolved"]["executor"], "cpu-raytrace");
        assert!(reply["resolved"]["profile"]["features"].as_array().unwrap().contains(&json!("refraction")));
        if command == "render.start" {
            let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
            // Cancellation can remove the job from the session's lookup immediately.
            // Retain its handle before cancelling; do not rely on a scheduling race.
            let handle = s.render_job_handle(id).unwrap();
            s.execute("render.cancel", &json!({"job":id})).unwrap();
            let status = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
            assert!(
                matches!(status.progress.phase, solidcraft_render::job::Phase::Cancelled | solidcraft_render::job::Phase::Completed),
                "phase {:?}, error {:?}; full status {status:?}",
                status.progress.phase,
                status.error
            );
        }
    }
    for boundary in ["closed_mesh", "thin_sheet"] {
        let mut unsupported = params.clone();
        unsupported["raytrace"]["dielectrics"][0]["boundary"] = json!(boundary);
        assert!(s.execute("render.preflight", &unsupported).is_err(), "a general closed mesh or a twelve-triangle sheet remains unsupported");
    }
    let mut opaque = params.clone();
    opaque["raytrace"]["dielectrics"] = json!([]);
    let reply = s.execute("render.preflight", &opaque).unwrap();
    assert_eq!(reply["refraction"], false);
    assert_eq!(reply["transparency"], "opaque_only");
    assert_eq!(reply["dielectric_boundaries"], json!([]));
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn dielectric_preflight_refuses_wrong_captured_identity_without_changing_design() {
    let host = CpuHost::for_platform().unwrap();
    let mut s = Session::default();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("solid.box", &json!({"length":20,"width":20,"height":2})).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len());
    let params = json!({"integrator":"raytrace","settings":{"width":1,"height":1},"raytrace":{"dielectrics":[{
        "body_index":0,"body_name":"wrong captured occurrence","boundary":"closed_box","ior":1.5,"absorption_per_mm":[0,0,0],"thickness_mm":0
    }]}});
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &params).unwrap_err().to_string();
        assert!(error.contains("identity mismatch"), "{error}");
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len()));
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[test]
fn registered_product_camera_fit_changes_only_view_and_keeps_orientation() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":20,"width":15,"height":10})).unwrap();
    let camera = solidcraft_render::Camera { yaw: 0.2, pitch: 0.3, roll: 0.1, ..Default::default() };
    s.execute("render.camera.set", &json!({"camera":camera})).unwrap();
    let doc = s.doc.to_json();
    let undo = s.undo.len();
    let inspected = s.execute("render.camera.get", &json!({})).unwrap();
    assert_eq!(inspected["camera"], json!(camera));
    assert_eq!(inspected["document_changed"], false);
    assert!(s.execute("render.camera.get", &json!({"fit":true})).is_err());
    let fitted = s.execute("render.camera.fit", &json!({})).unwrap();
    assert_eq!(fitted["document_changed"], false);
    let after = s.camera.unwrap();
    assert_eq!((after.yaw, after.pitch, after.roll), (camera.yaw, camera.pitch, camera.roll));
    // A picture taller than wide moves the camera back by the ratio, keeping the orientation
    // and the target; a wide one is the plain fit.
    let tall = s.execute("render.camera.fit", &json!({"aspect":0.5})).unwrap();
    assert_eq!(tall["document_changed"], false);
    let back = s.camera.unwrap();
    assert!((back.distance - 2.0 * after.distance).abs() < 1e-9, "{} vs {}", back.distance, after.distance);
    assert_eq!((back.yaw, back.pitch, back.roll, back.target), (after.yaw, after.pitch, after.roll, after.target));
    s.execute("render.camera.fit", &json!({"aspect":2.0})).unwrap();
    assert_eq!(s.camera.unwrap(), after);
    assert_eq!(s.doc.to_json(), doc);
    assert_eq!(s.undo.len(), undo);
    // Hostile aspects are refused and leave the finite camera, the document and undo alone:
    // a subnormal one would otherwise divide the fitted distance into infinity.
    for aspect in [json!(0), json!(-1), json!(1e-320), json!(1e-7), json!(1e7), json!("tall")] {
        assert!(s.execute("render.camera.fit", &json!({"aspect":aspect})).is_err(), "aspect {aspect}");
        assert_eq!(s.camera.unwrap(), after);
    }
    assert!(s.execute("render.camera.fit", &json!({"scale":0})).is_err());
    assert_eq!(s.doc.to_json(), doc);
    assert_eq!(s.undo.len(), undo);
}

#[test]
fn explicit_render_camera_preserves_orientation_and_refuses_invalid_state_atomically() {
    let mut s = Session::default();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len(), s.model.is_stale());
    for up in [solidcraft_render::UpAxis::Z, solidcraft_render::UpAxis::Y] {
        let camera = solidcraft_render::Camera { roll: 0.41, yaw: -0.3, pitch: 0.5, up, ..Default::default() };
        let reply = s.execute("render.camera.set", &json!({"camera":camera})).unwrap();
        assert_eq!(s.camera, Some(camera));
        assert_eq!(reply["camera"], json!(camera));
        assert_eq!(reply["document_changed"], false);
        for (key, value) in [
            ("roll", json!(null)),
            ("roll", json!("invalid")),
            ("roll", json!(1e7)),
            ("up", json!("invalid")),
            ("up", json!(null)),
            ("distance", json!(0)),
            ("distance", json!(1e10)),
            ("fov", json!(-1)),
            ("fov", json!(4)),
            ("target", json!([1e10, 0, 0])),
            ("unknown", json!(true)),
        ] {
            let mut invalid = json!({"camera":camera});
            invalid["camera"][key] = value;
            assert!(s.execute("render.camera.set", &invalid).is_err(), "{invalid}");
            assert_eq!(s.camera, Some(camera));
        }
        for invalid in [json!({}), json!([]), json!({"camera":camera,"width":1}), json!({"camera":null})] {
            assert!(s.execute("render.camera.set", &invalid).is_err(), "{invalid}");
            assert_eq!(s.camera, Some(camera));
        }
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len(), s.model.is_stale()));
    }
    s.execute("render.camera.set", &json!({"camera":{"target":[0,0,0],"yaw":0.2,"pitch":0.3,"distance":100,"fov":0}})).unwrap();
    assert_eq!(s.camera.unwrap().roll, 0.0);
    assert_eq!(s.camera.unwrap().up, solidcraft_render::UpAxis::Z);
    // The Design API still rejects explicit up, as specified by upstream #5094.
    let before_camera = s.camera;
    assert!(s.execute("view.apply", &json!({"camera":before_camera.unwrap()})).is_err());
    assert_eq!(s.camera, before_camera);
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.is_dirty(), s.undo.len(), s.redo.len(), s.model.is_stale()));
}

/// #5089: a setup saved with exposure (the render workspace and the other executors use it)
/// renders on the raster executor, which has no exposure, and the reply says what it left out.
#[test]
fn a_setup_saved_with_exposure_renders_and_names_the_controls_it_leaves_out() {
    let (mut s, host) = session();
    s.execute("render.settings.set", &json!({"name":"x","set":{"mode":"preview","width":16,"height":16,"exposure_ev":0.3}})).unwrap();
    let reply = s.execute("render.preflight", &json!({"setup":"x"})).unwrap();
    assert_eq!(reply["ignored_controls"], json!(["exposure_ev"]));
    let plain = s.execute("render.preflight", &request()).unwrap();
    assert_eq!(plain["ignored_controls"], json!([]));
    let started = s.execute("render.start", &json!({"setup":"x"})).unwrap();
    assert_eq!(started["ignored_controls"], json!(["exposure_ev"]));
    finish(&host);
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    assert_eq!(s.execute("render.status", &json!({"job":id})).unwrap()["progress"]["phase"], "completed");
}

#[test]
fn output_viewer_copy_validates_completed_job_without_mutation_or_pixel_payloads() {
    let (mut s, host) = session();
    let job = start(&mut s);
    assert!(s.execute("render.result.copy", &json!({"job":job})).is_err());
    finish(&host);
    let document = s.doc.clone();
    let before = (s.revision, s.undo.len(), s.redo.len(), host.admitted_bytes());
    let copied = s.execute("render.result.copy", &json!({"job":job})).unwrap();
    assert_eq!(copied["frame"]["job"], json!(job));
    assert_eq!(copied["frame"]["width"], 16);
    assert!(copied.get("png").is_none());
    for params in [json!({}), json!(null)] {
        assert_eq!(s.execute("render.result.copy", &params).unwrap(), copied, "an omitted job uses the selected result");
    }
    for params in
        [json!({"job":1e300}), json!({"job":"selected"}), json!({"job":job,"extra":true}), json!({"job":{"session":0,"serial":0,"generation":0}})]
    {
        assert!(s.execute("render.result.copy", &params).is_err());
    }
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.undo.len(), s.redo.len(), host.admitted_bytes()));
}
