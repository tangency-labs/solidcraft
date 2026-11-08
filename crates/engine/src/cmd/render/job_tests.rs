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
            "lighting":{"environment":[0.1,0.3,0.6],"sun":{"direction":[1,-1,-1],"angular_radius_rad":0.05,"radiance":[2,3,4]},"area_lights":[],"directional_lights":[
                {"direction":[0,0,2],"color":[0.25,0.5,1],"intensity":0},
                {"direction":[3,-4,5],"color":[0.6,0.4,0.2],"intensity":2.5}],"punctual_lights":[
                {"position_mm":[1.25,-2.5,4.75],"color":[0.25,0.5,1],"intensity":2,"reference_distance_mm":4.5,"minimum_distance_mm":0.002},
                {"position_mm":[4,5,6],"color":[1,0.5,0.25],"intensity":3,"reference_distance_mm":7,"minimum_distance_mm":0.003,
                 "cone":{"emitted_axis":[-4,-5,-6],"inner_half_angle_radians":0.2,"outer_half_angle_radians":0.7}}]},
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
    let captured_options: JobOptions = serde_json::from_value(checked["options"].clone()).unwrap();
    assert_eq!(checked["retained_options_bytes"], json!(captured_options.retained_bytes().unwrap()));
    assert_eq!(captured_options.lighting.unwrap().directional_lights.iter().count(), 2);
    let sources: Vec<_> = captured_options.lighting.unwrap().punctual_lights.iter().copied().collect();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].position_mm, [1.25, -2.5, 4.75]);
    assert_eq!(sources[0].reference_distance_mm, 4.5);
    assert_eq!(sources[0].minimum_distance_mm, 0.002);
    assert_eq!(
        sources[1].cone,
        Some(solidcraft_render::job::SpotConeOptions {
            emitted_axis: [-4.0, -5.0, -6.0],
            inner_half_angle_radians: 0.2,
            outer_half_angle_radians: 0.7
        })
    );
    let saved_lighting = match &saved.job {
        Some(solidcraft_doc::render_settings::RenderJob::GpuPath { options, .. }) => options.lighting.unwrap(),
        _ => panic!("GPU"),
    };
    assert_eq!(captured_options.lighting, Some(saved_lighting), "portable options reach the exact runtime capture unchanged");
    assert_eq!(checked["options"]["lighting"]["directional_lights"][0]["intensity"].as_f64(), Some(0.0));

    assert_eq!(checked["options"]["denoise"], true);
    assert_eq!(checked["environment"]["rotation_rad"], 0.75);
    assert_eq!(checked["transparency"], "coverage_alpha");
    assert_eq!(checked["request"]["transparent_background"], true);
    // Protocol metadata follows the actual captured output flag; this executor
    // fixture does not claim physical GPU image quality or material transmission.
    let mut opaque = serde_json::to_value(&saved).unwrap();
    opaque["job"]["transparent_background"] = json!(false);
    let opaque = s.execute("render.preflight", &json!({"settings":opaque})).unwrap();
    assert_eq!(opaque["transparency"], "opaque_only");
    assert_eq!(opaque["request"]["transparent_background"], false);
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
        let retained = preflight["retained_request_bytes"].as_u64().unwrap();
        let request: solidcraft_render::job::ImageRequest = serde_json::from_value(preflight["request"].clone()).unwrap();
        assert!(request.raytrace.is_none() && request.environment.is_none() && request.textures.is_none());
        assert_eq!(retained, solidcraft_render::job::ImageRequest::fixed_cpu_metadata_bytes());
        let started = s.execute("render.start", &params).unwrap();
        let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
        let handle = s.render_job_handle(id).unwrap();
        s.execute("render.cancel", &json!({"job":id})).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Cancelled);
        // Terminal phase precedes actual work/proof release. The retained request
        // belongs to Record and must remain funded while its handle is observed.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !handle.work_released() {
            assert!(std::time::Instant::now() < deadline, "worker did not release cancelled work/proof backing");
            std::thread::yield_now();
        }
        assert!(handle.frame().unwrap().is_none() && handle.preview().unwrap().is_none());
        assert_eq!(raytrace_host.admitted_bytes(), retained, "only the exact retained request metadata remains");
        assert!(s.render_retained_results().unwrap().is_empty(), "collecting a cancelled job cannot create a completed image");
        assert!(s.render_job_handle(id).is_err(), "the cancelled live registry handle was collected");
        let historical = s.execute("render.status", &json!({"job":id})).unwrap();
        assert_eq!(historical["progress"]["phase"], "cancelled");
        assert_eq!(raytrace_host.admitted_bytes(), retained, "the external handle still owns its request reservation");
        drop(handle);
        while raytrace_host.admitted_bytes() != 0 {
            assert!(std::time::Instant::now() < deadline, "last cancelled Record did not release retained request metadata");
            std::thread::yield_now();
        }
        // Plain status/history metadata does not retain Record or its lease.
        assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Cancelled);
        assert_eq!(historical["progress"]["phase"], "cancelled");
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
fn saved_cpu_output_runs_exports_and_regrades_its_exact_retained_native_master() {
    use solidcraft_render::job::Phase;
    let mut s = Session::default();
    s.execute(
        "render.settings.set",
        &json!({"name":"CPU output","set":{"width":8,"height":8,"exposure_ev":1.5,
        "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":81},"lights":[],
            "output":{"tone_map":"aces_fitted","finishing":{"rgb_gain":[1.2,0.8,1.1],"photographic":{"grain":0.03,"seed":42},
                "bloom":{"strength":0.2,"threshold":0.5,"radius":1}},"backdrop":{"mode":"radial"},"linear_exr":true}}}}}),
    )
    .unwrap();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let request = json!({"setup":"CPU output"});
    let checked = s.execute("render.preflight", &request).unwrap();
    assert_eq!(checked["resolved"]["executor"], "cpu-raytrace");
    assert_eq!(checked["request"]["raytrace"]["exposure_ev"], 1.5);
    assert_eq!(checked["request"]["raytrace"]["output"]["linear_exr"], true);
    let reply = s.execute("render.start", &request).unwrap();
    let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap().progress.phase, Phase::Completed);
    let frame = handle.frame().unwrap().unwrap();
    let master = frame.linear_exr().unwrap().to_vec();
    let path = std::env::temp_dir().join(format!("solidcraft-cpu-master-{}-{}.exr", std::process::id(), id.session));
    s.execute("render.export", &json!({"job":id,"path":path,"format":"exr"})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), master);
    std::fs::remove_file(path).unwrap();
    let derivative = s.execute("render.result.regrade", &json!({"job":id,"presentation":{"exposure_ev":-2,"background":[3,7,11]}})).unwrap();
    let derivative: JobId = serde_json::from_value(derivative["job"].clone()).unwrap();
    let derivative = s.render_job_handle(derivative).unwrap();
    assert_eq!(derivative.wait_timeout(std::time::Duration::from_secs(30)).unwrap().progress.phase, Phase::Completed);
    assert_eq!(derivative.frame().unwrap().unwrap().linear_exr().unwrap(), master);
    assert_eq!(frame.linear_exr().unwrap(), master);
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_output_envelope_refuses_before_hdr_read_or_scene_capture() {
    let mut s = Session::default();
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    let before = (s.revision, s.evaluations, s.undo.len());
    let request = json!({"integrator":"raytrace","settings":{"width":16384,"height":1},
        "raytrace":{"budget":"workstation","output":{"linear_exr":true}},"environment":{"path":"unopened-output.hdr"}});
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &request).unwrap_err().to_string();
        assert!(error.contains("linear EXR"), "{error}");
        assert!(!error.contains("unopened-output.hdr"), "an incompatible output must refuse before source IO");
        assert_eq!(before, (s.revision, s.evaluations, s.undo.len()));
    }
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
    camera.target = solidcore_record::Vec3::new(scene.bounds.min.x, center.y, center.z);
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

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_physical_lens_saved_and_runtime_requests_share_one_captured_camera() {
    let camera = solidcraft_render::Camera { fov: 0.7, distance: 20.0, roll: 0.3, ..Default::default() };
    let lens = solidcraft_render::job::ThinLens { aperture_radius_mm: 0.5, focus_distance_mm: 20.0 };
    let mut s = Session::default();
    s.execute(
        "render.settings.set",
        &json!({"name":"Focus","set":{"width":8,"height":8,"camera":camera,"exposure_ev":1.5,
        "job":{"method":"cpu_raytrace","options":{"lens":lens,"progressive":{"samples_per_pixel":3,"seed":81},"lights":[]}}}}),
    )
    .unwrap();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let saved = s.execute("render.preflight", &json!({"setup":"Focus"})).unwrap();
    let runtime = s
        .execute(
            "render.preflight",
            &json!({"settings":{"width":8,"height":8,"camera":camera},"integrator":"raytrace",
        "raytrace":{"lens":lens,"exposure_ev":1.5,"progressive":{"samples_per_pixel":3,"seed":81},"lights":[]}}),
        )
        .unwrap();
    assert_eq!(saved["request"], runtime["request"]);
    assert_eq!(saved["request"]["camera"], json!(camera));
    assert_eq!(saved["request"]["raytrace"]["lens"], json!(lens));
    let started = s.execute("render.start", &json!({"setup":"Focus"})).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(job).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap().progress.phase, solidcraft_render::job::Phase::Completed);
    assert!(handle.frame().unwrap().is_some());
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_lens_refuses_incompatible_sampling_and_camera_before_hdr_or_scene_capture() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for fixed in [false, true] {
        let camera = solidcraft_render::Camera { fov: if fixed { 0.7 } else { 0.0 }, ..Default::default() };
        let mut request = json!({"integrator":"raytrace","settings":{"width":8,"height":8,"camera":camera},
            "raytrace":{"lens":{"aperture_radius_mm":0.5,"focus_distance_mm":20.0},"progressive":{"samples_per_pixel":3}},
            "environment":{"path":"must-not-open-cpu-lens.hdr"}});
        if fixed {
            request["raytrace"].as_object_mut().unwrap().remove("progressive");
        }
        for command in ["render.preflight", "render.start"] {
            let error = s.execute(command, &request).unwrap_err().to_string();
            assert!(error.contains(if fixed { "progressive" } else { "perspective" }), "{error}");
            assert!(!error.contains("must-not-open-cpu-lens.hdr"));
            assert_eq!(host.admitted_bytes(), 0);
            assert!(Arc::ptr_eq(&document, &s.doc));
            assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
        }
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_alpha_refuses_opaque_only_output_before_hdr_io_or_scene_capture() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for output in [json!({"backdrop":{}}), json!({"finishing":{"bloom":{"strength":1}}}), json!({"finishing":{"photographic":{"grain":0.03}}})] {
        let request = json!({"integrator":"raytrace","transparent_background":true,"settings":{"width":8,"height":8,"camera":solidcraft_render::Camera::default()},
            "raytrace":{"output":output},"environment":{"path":"must-not-open-cpu-alpha.hdr"}});
        for command in ["render.preflight", "render.start"] {
            let error = s.execute(command, &request).unwrap_err().to_string();
            assert!(error.contains("opaque"), "{error}");
            assert!(!error.contains("must-not-open-cpu-alpha.hdr"));
            assert_eq!(host.admitted_bytes(), 0);
            assert!(Arc::ptr_eq(&document, &s.doc));
            assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), before);
        }
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_denoise_reports_its_actual_native_request_and_keeps_regrade_raw() {
    use solidcraft_render::job::Phase;
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":3,"body_name":"Filtered"})).unwrap();
    s.execute(
        "render.settings.set",
        &json!({"name":"CPU denoise","set":{"width":8,"height":8,
        "job":{"method":"cpu_raytrace","options":{"denoise":true,"progressive":{"samples_per_pixel":3,"seed":81},"output":{"linear_exr":true}}}}}),
    )
    .unwrap();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &json!({"setup":"CPU denoise"})).unwrap();
    assert_eq!(checked["resolved"]["executor"], "cpu-raytrace");
    assert_eq!(checked["request"]["raytrace"]["denoise"], true);
    assert_eq!(checked["denoising"]["backend"], "native_cpu");
    assert_eq!(checked["denoising"]["passes"], 3);
    assert_eq!(checked["denoising"]["raw_linear_accumulator_preserved"], true);
    let started = s.execute("render.start", &json!({"setup":"CPU denoise"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
    assert_eq!(json!(status.request), checked["request"]);
    let frame = handle.frame().unwrap().unwrap();
    let raw = frame.linear_exr().unwrap().to_vec();
    let result = s.execute("render.result.regrade", &json!({"job":id,"presentation":{"exposure_ev":-1.0}})).unwrap();
    let derivative: JobId = serde_json::from_value(result["job"].clone()).unwrap();
    let derivative = s.render_job_handle(derivative).unwrap();
    let finished = derivative.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(finished.progress.phase, Phase::Completed);
    assert!(!finished.presentation.as_ref().unwrap().denoise_applied, "regrade never pretends to reconstruct absent geometry guides");
    assert_eq!(derivative.frame().unwrap().unwrap().linear_exr().unwrap(), raw);
    assert_eq!(frame.linear_exr().unwrap(), raw);
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), before);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_optics_match_runtime_capture_pixels_prefixes_cancel_and_accounting() {
    use solidcraft_render::job::{CancelToken, ImageRequest, Phase};
    use solidcraft_render::raytrace::image::{ImagePoll, ImageTask};
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":3,"body_name":"Glass"})).unwrap();
    let camera = solidcraft_render::Camera {
        target: solidcore_record::Vec3::new(5.0, 5.0, 1.5),
        distance: 20.0,
        yaw: 0.3,
        pitch: 0.4,
        fov: 0.5,
        ..Default::default()
    };
    s.execute("render.settings.set",&json!({"name":"CPU glass","set":{"camera":camera,"width":3,"height":3,"background":[255,255,255],"exposure_ev":1.0,
        "job":{"method":"cpu_raytrace","options":{"lights":[],"environment":{"color":[1,1,1],"intensity":1},"progressive":{"samples_per_pixel":3,"seed":81}}}}})).unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"CPU glass","body_index":0,"body_name":"Glass","expected_revision":s.revision,
        "optics":{"ior":1.51,"absorption_per_mm":[0.1,0.02,0.0],"boundary":"closed_mesh"}}),
    )
    .unwrap();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let saved = s.execute("render.preflight", &json!({"setup":"CPU glass"})).unwrap();
    let request: ImageRequest = serde_json::from_value(saved["request"].clone()).unwrap();
    let runtime=s.execute("render.preflight",&json!({"settings":{"camera":camera,"width":3,"height":3,"background":[255,255,255]},"integrator":"raytrace","raytrace":request.raytrace})).unwrap();
    assert_eq!(saved["request"], runtime["request"]);
    let scene = crate::product_scene::extract(&s).unwrap();
    let saved_dielectrics = super::optical_bindings::resolve_cpu(&s, &scene, &s.doc.render.setups["CPU glass"].optics, "check").unwrap();
    let runtime_dielectrics = &request.raytrace.as_ref().unwrap().dielectrics;
    assert_eq!(saved_dielectrics, *runtime_dielectrics);
    // Saved lowering reserves exactly its targets; hostile runtime input reserves
    // the bounded eight-target array. Equal serialized intent need not have equal
    // allocation capacity. Prove the entire difference from the actual captures.
    assert_eq!(saved_dielectrics.capacity(), saved_dielectrics.len());
    assert_eq!(runtime_dielectrics.capacity(), 8);
    for (saved_input, runtime_input) in saved_dielectrics.iter().zip(runtime_dielectrics) {
        assert_eq!(saved_input.body_name.capacity(), runtime_input.body_name.capacity());
    }
    let extra_slots = runtime_dielectrics.capacity() - saved_dielectrics.capacity();
    let extra_bytes = (extra_slots * std::mem::size_of::<solidcraft_render::job::DielectricOptions>()) as u64;
    assert_eq!(saved["retained_request_bytes"].as_u64().unwrap() + extra_bytes, runtime["retained_request_bytes"].as_u64().unwrap());
    // Two options copies belong to image work; a third belongs to the retained
    // immutable request. All other admission fields must remain exactly equal.
    let mut expected_runtime_admission = saved["admission"].clone();
    for (field, copies) in [("preparation_bytes", 2), ("estimated_total_bytes", 3)] {
        expected_runtime_admission[field] = json!(saved["admission"][field].as_u64().unwrap() + copies * extra_bytes);
    }
    assert_eq!(expected_runtime_admission, runtime["admission"]);
    assert_eq!(saved["refraction"], true);
    let original_mesh = scene.meshes[0].mesh.clone();
    assert!(!scene.has_transmission(), "saved CPU lowering leaves the captured product appearance and physical material unchanged");
    let complete = |request: ImageRequest, quantum| {
        let options = request.raytrace.clone().unwrap();
        let mut task = ImageTask::new(scene.clone(), request, CancelToken::default(), options).unwrap();
        let mut previews = Vec::new();
        for _ in 0..200000 {
            match task.step_with_previews(quantum).unwrap() {
                ImagePoll::Pending(_) => {}
                ImagePoll::Preview { png, samples_per_pixel } => previews.push((samples_per_pixel, png)),
                ImagePoll::Complete(png) => return (previews, png),
            }
        }
        panic!("bounded saved-optics producer did not finish")
    };
    let (previews, png) = complete(request.clone(), 1);
    assert_eq!(previews.iter().map(|p| p.0).collect::<Vec<_>>(), [1, 2]);
    assert_eq!((previews.clone(), png.clone()), complete(request.clone(), 16384));
    for (samples, preview) in previews {
        let mut prefix = request.clone();
        prefix.raytrace.as_mut().unwrap().progressive.as_mut().unwrap().samples_per_pixel = samples;
        assert_eq!(complete(prefix, 3).1, preview);
    }
    let cancel = CancelToken::default();
    let mut interrupted = ImageTask::new(scene.clone(), request.clone(), cancel.clone(), request.raytrace.clone().unwrap()).unwrap();
    let mut checkpoint = false;
    for _ in 0..200000 {
        if matches!(interrupted.step_with_previews(1).unwrap(), ImagePoll::Preview { .. }) {
            checkpoint = true;
            break;
        }
    }
    assert!(checkpoint, "cancel exercises a real complete saved-optics checkpoint");
    cancel.cancel();
    assert_eq!(interrupted.step_with_previews(1).err().unwrap().code, solidcraft_render::job::ErrorCode::Cancelled);
    assert!(interrupted.take_linear_exr().is_none());
    let started = s.execute("render.start", &json!({"setup":"CPU glass"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let terminal = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(terminal.progress.phase, Phase::Completed, "{:?}", terminal.error);
    assert_eq!(handle.frame().unwrap().unwrap().png(), png);
    assert!(Arc::ptr_eq(&original_mesh, &crate::product_scene::extract(&s).unwrap().meshes[0].mesh));
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_saved_closed_optics_do_not_broaden_geometry_or_guess_stale_targets() {
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius":5,"height":3,"body_name":"Curved"})).unwrap();
    s.execute("render.settings.set", &json!({"name":"CPU glass","set":{"width":3,"height":3,"job":{"method":"cpu_raytrace"}}})).unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"CPU glass","body_index":0,"body_name":"Curved","expected_revision":s.revision,"optics":{"ior":1.5,"boundary":"closed_mesh"}}),
    )
    .unwrap();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &json!({"setup":"CPU glass"})).unwrap_err().to_string();
        // The finer main147 tessellation reaches the exact axis-plane guard.
        assert!(error.starts_with("InvalidScene:") && error.contains("box face is not axis aligned"), "{error}");
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(host.admitted_bytes(), 0);
    }
    s.execute("browser.visibility", &json!({"items":["Curved"],"visible":false})).unwrap();
    let error = s.execute("render.preflight", &json!({"setup":"CPU glass"})).unwrap_err().to_string();
    assert!(error.contains("hidden or absent"), "{error}");
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_saved_thin_sheet_uses_its_authored_ior_beer_thickness_and_roundtrips_offline() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("solidcraft-cpu-saved-sheet-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let source = dir.join("Sheet.obj");
    std::fs::write(&source, "o Sheet\nv -10 -10 0\nv 10 -10 0\nv 10 10 0\nv -10 10 0\nf 1 2 3\nf 1 3 4\n").unwrap();
    let mut s = Session::default();
    s.execute("file.insert_mesh", &json!({"path":source})).unwrap();
    let scene = s.execute("render.scene", &json!({})).unwrap();
    let name = scene["bodies"][0]["name"].as_str().unwrap().to_owned();
    let mut camera = solidcraft_render::Camera { distance: 10.0, ..Default::default() };
    camera.set_view(solidcraft_render::StandardView::Top);
    s.execute(
        "render.settings.set",
        &json!({"name":"Sheet","set":{"camera":camera,"width":1,"height":1,"job":{"method":"cpu_raytrace","options":{
        "lights":[],"environment":{"color":[1,1,1],"intensity":1},"progressive":{"samples_per_pixel":3,"seed":81}}}}}),
    )
    .unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"Sheet","body_index":0,"body_name":name,"expected_revision":s.revision,
        "optics":{"boundary":"thin_sheet","thickness_mm":2.0,"ior":1.51,"absorption_per_mm":[0.1,0.2,0.0],"shadow_transmission":false}}),
    )
    .unwrap();
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    s = Session::new(reopened);
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    let before = s.doc.clone();
    let checked = s.execute("render.preflight", &json!({"setup":"Sheet"})).unwrap();
    assert_eq!(checked["request"]["raytrace"]["dielectrics"][0]["boundary"], "thin_sheet");
    assert_eq!(checked["request"]["raytrace"]["dielectrics"][0]["thickness_mm"], 2.0);
    let started = s.execute("render.start", &json!({"setup":"Sheet"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
    let frame = handle.frame().unwrap().unwrap();
    let decoded = image::load_from_memory(frame.png()).unwrap().to_rgba8();
    let f = ((1.0f64 - 1.51) / (1.0 + 1.51)).powi(2);
    let rgb = [0.1f64, 0.2, 0.0].map(|sigma| {
        let a = (-sigma * 2.0).exp();
        let denominator = 1.0 - f * f * a * a;
        let scene = f + (1.0 - f).powi(2) * (f * a * a + a) / denominator;
        solidcraft_color::srgb::encode8(scene / (1.0 + scene))
    });
    assert_eq!(decoded.get_pixel(0, 0).0, [rgb[0], rgb[1], rgb[2], 255]);
    assert!(Arc::ptr_eq(&before, &s.doc));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_path_saved_and_runtime_captures_execute_without_mutating_document_or_history() {
    let mut s = Session::default();
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    let camera = solidcraft_render::Camera::default();
    s.execute("render.settings.set",&json!({"name":"Path","set":{"width":8,"height":8,"camera":camera,"job":{"method":"cpu_raytrace","options":{"path":{"max_depth":3},"progressive":{"samples_per_pixel":3,"seed":9},"lights":[]}}}})).unwrap();
    let saved = s.execute("render.preflight", &json!({"setup":"Path"})).unwrap();
    let runtime=s.execute("render.preflight",&json!({"settings":{"width":8,"height":8,"camera":camera},"integrator":"raytrace","raytrace":{"path":{"max_depth":3},"progressive":{"samples_per_pixel":3,"seed":9},"lights":[]}})).unwrap();
    assert_eq!(saved["request"], runtime["request"]);
    assert_eq!(saved["request"]["raytrace"]["path"]["max_depth"], 3);
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let started = s.execute("render.start", &json!({"setup":"Path"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    assert_eq!(handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap().progress.phase, solidcraft_render::job::Phase::Completed);
    assert!(handle.frame().unwrap().is_some());
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}
#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_path_refuses_fixed_or_shadowless_transport_before_hdr_io() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for fixed in [false, true] {
        let mut request = json!({"integrator":"raytrace","settings":{"width":8,"height":8,"camera":solidcraft_render::Camera::default()},"raytrace":{"path":{"max_depth":3},"progressive":{"samples_per_pixel":3},"shadows":false},"environment":{"path":"must-not-open-path.hdr"}});
        if fixed {
            request["raytrace"].as_object_mut().unwrap().remove("progressive");
            request["raytrace"]["shadows"] = json!(true);
        }
        for command in ["render.preflight", "render.start"] {
            let error = s.execute(command, &request).unwrap_err().to_string();
            assert!(error.contains(if fixed { "progressive" } else { "shadows" }), "{error}");
            assert!(!error.contains("must-not-open-path.hdr"));
            assert_eq!(host.admitted_bytes(), 0);
            assert!(Arc::ptr_eq(&document, &s.doc));
            assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
        }
    }
}

#[test]
fn composed_saved_cpu_path_and_subsurface_refuse_before_hdr_resources_or_scene_capture() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":3,"body_name":"Glass"})).unwrap();
    s.execute(
        "render.settings.set",
        &json!({"name":"Glass","set":{"width":8,"height":8,
        "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":81}}}}}),
    )
    .unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"Glass","body_index":0,"body_name":"Glass","expected_revision":s.revision,
        "optics":{"ior":1.51,"boundary":"closed_box","shadow_transmission":false}}),
    )
    .unwrap();
    let mut settings = serde_json::to_value(&s.doc.render.setups["Glass"]).unwrap();
    settings["job"]["transparent_background"] = json!(true);
    settings["job"]["options"]["path"] = json!({"max_depth":3});
    settings["job"]["options"]["denoise"] = json!(true);
    settings["job"]["options"]["output"] = json!({"linear_exr":true});
    settings["job"]["options"]["hdr"] = json!({"resource":"0".repeat(64),"rotation_rad":0,"intensity":1,"samples":16});
    settings["optics"][0]["optics"]["subsurface"] = json!({"scattering_per_mm":0.1,"anisotropy":0});
    let authored = settings.clone();
    let document = s.doc.clone();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for command in ["render.preflight", "render.start", "render.settings.set"] {
        let request = if command == "render.settings.set" { json!({"name":"Invalid","set":settings}) } else { json!({"settings":settings}) };
        let error = s.execute(command, &request).unwrap_err().to_string();
        assert!(error.contains("subsurface"), "{command}: {error}");
        assert!(!error.contains("missing embedded HDR"), "compatibility must precede resource access: {error}");
        assert_eq!(settings, authored);
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), history);
        assert!(s.render_retained_results().unwrap().is_empty());
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn composed_runtime_cpu_path_and_closed_shadow_approximation_refuse_before_missing_hdr_io() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":3,"body_name":"Glass"})).unwrap();
    let request = json!({"integrator":"raytrace","settings":{"width":8,"height":8},"transparent_background":true,
        "raytrace":{"path":{"max_depth":3},"denoise":true,"progressive":{"samples_per_pixel":3,"seed":81},
            "output":{"linear_exr":true},"dielectrics":[{"body_index":0,"body_name":"Glass","ior":1.51,"boundary":"closed_mesh","shadow_transmission":true,"absorption_per_mm":[0,0,0],"thickness_mm":0}]},
        "environment":{"path":"must-not-open-composed-optics.hdr"}});
    let authored = request.clone();
    let document = s.doc.clone();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &request).unwrap_err().to_string();
        assert!(error.contains("dielectric"), "{error}");
        assert!(!error.contains("must-not-open-composed-optics.hdr"));
        assert_eq!(host.admitted_bytes(), 0);
        assert_eq!(request, authored);
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), history);
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_stage_preflight_describes_actual_opt_in_path_transport_without_changing_the_physical_stage() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":3,"body_name":"Subject"})).unwrap();
    s.execute(
        "render.settings.set",
        &json!({"name":"Stage","set":{"width":8,"height":8,
        "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":81},"lights":[]}}}}),
    )
    .unwrap();
    s.execute("render.stage.ground", &json!({"name":"Stage"})).unwrap();
    let saved = s.doc.render.setups["Stage"].clone();
    assert_eq!(saved.stage.len(), 1);
    let product = crate::product_scene::extract(&s).unwrap();
    let product_triangles: usize = product.meshes.iter().map(|body| body.mesh.triangles.len()).sum();
    assert!(product_triangles > 0);
    let document = s.doc.clone();
    let model = s.model.state();
    let history = (s.revision, s.undo.len(), s.redo.len());
    let direct = s.execute("render.preflight", &json!({"setup":"Stage"})).unwrap();
    let mut path = saved.clone();
    let Some(solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = &mut path.job else { panic!("CPU fixture") };
    options.path = Some(solidcraft_render::job::CpuPathOptions { max_depth: 3 });
    let indirect = s.execute("render.preflight", &json!({"settings":path})).unwrap();
    assert_eq!(direct["stage_transport"], "CPU direct/specular only; no diffuse interreflection");
    assert_eq!(indirect["stage_transport"], "CPU finite-depth opaque diffuse/specular path lighting");
    for reply in [&direct, &indirect] {
        assert_eq!(reply["resolved"]["executor"], "cpu-raytrace");
        assert_eq!(reply["stage_panels"], 1);
        assert_eq!(
            reply["admission"]["triangles"].as_u64(),
            Some(product_triangles as u64 + 2),
            "the same real finite ground is present in each captured scene"
        );
        assert_eq!(reply["request"]["camera"], serde_json::to_value(saved.camera).unwrap());
        assert_eq!(reply["scene_generation"], s.revision);
    }
    assert!(direct["request"]["raytrace"].get("path").is_none());
    assert_eq!(indirect["request"]["raytrace"]["path"]["max_depth"], 3);
    assert_eq!(s.doc.render.setups["Stage"], saved);
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!((s.revision, s.undo.len(), s.redo.len()), history);
    assert!(s.render_retained_results().unwrap().is_empty());
    assert_eq!(host.admitted_bytes(), 0);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_aging_cold_reopen_resolves_exact_target_and_changes_real_png_without_engineering_edits() {
    use solidcraft_doc::render_aging::SavedAgingStack;
    use solidcraft_render::job::aging::{AgingKind, AgingLayer};
    let mut s = Session::default();
    s.execute("solid.box", &json!({"body_name":"Weathered panel","length":10,"width":10,"height":2})).unwrap();
    let targets = super::surface_bindings::targets(&s, "test").unwrap();
    let (target, name) = targets.iter().find(|(_, name)| name.as_str() == "Weathered panel").unwrap();
    let name = name.clone();
    let mut layers = SavedAgingStack::default();
    layers
        .push(AgingLayer {
            kind: AgingKind::Dust,
            strength: 1.0,
            coverage: 1.0,
            upward_bias: 0.0,
            color_srgb: [255, 0, 0],
            metallic: 0.0,
            roughness: 1.0,
            ..Default::default()
        })
        .unwrap();
    layers.get_mut(0).unwrap().target = Some(target.clone());
    let physical = s.execute("component.properties", &json!({})).unwrap();
    let mut camera = solidcraft_render::Camera { distance: 15.0, target: solidcore_record::Vec3::new(5.0, 5.0, 1.0), fov: 0.0, ..Default::default() };
    camera.set_view(solidcraft_render::StandardView::Top);
    s.execute("render.settings.set", &json!({"name":"CPU aging","set":{"width":8,"height":8,"camera":camera,
        "job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":81},"aging":layers,"denoise":true,"output":{"linear_exr":true},"lights":[{"direction":[0,0,1],"color":[1,1,1],"intensity":std::f64::consts::PI}]}}}})).unwrap();
    let saved = s.doc.render.setups["CPU aging"].clone();
    let native = solidcraft_io::write_design_container(&s.doc).unwrap();
    s = Session::new(solidcraft_io::read_design(&native).unwrap());
    assert_eq!(s.doc.render.setups["CPU aging"], saved);
    s.set_render_job_host(Some(CpuHost::for_platform().unwrap())).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty());
    let checked = s.execute("render.preflight", &json!({"setup":"CPU aging"})).unwrap();
    assert_eq!(checked["request"]["raytrace"]["aging"][0]["target"]["body_name"], name);
    assert_eq!(checked, s.execute("render.preflight", &json!({"settings":saved})).unwrap());
    let started = s.execute("render.start", &json!({"setup":"CPU aging"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
    assert_eq!(json!(status.request), checked["request"]);
    let frame = handle.frame().unwrap().unwrap();
    assert!(frame.linear_exr().is_some());
    let image = image::load_from_memory(frame.png()).unwrap().to_rgba8();
    assert!(
        image.pixels().any(|pixel| u16::from(pixel[0]) > u16::from(pixel[1]) + 20 && u16::from(pixel[0]) > u16::from(pixel[2]) + 20),
        "actual captured CPU aging must colour the visible panel"
    );
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty()));
    assert_eq!(s.execute("component.properties", &json!({})).unwrap(), physical);
    assert_eq!(s.doc.render.setups["CPU aging"], saved);
    let mut stale = saved;
    let solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options, .. } = stale.job.as_mut().unwrap() else { panic!("CPU fixture") };
    options.aging.get_mut(0).unwrap().target.as_mut().unwrap().feature += 100;
    for command in ["render.preflight", "render.start"] {
        let error = s.execute(command, &json!({"settings":stale})).unwrap_err().to_string();
        assert!(error.contains("rebinding"), "{error}");
        assert!(Arc::ptr_eq(&document, &s.doc));
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_aging_mixed_stable_runtime_and_anisotropic_intent_refuse_before_hdr_io_or_scene_capture() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let layer = json!({"kind":"dust"});
    let base = json!({"integrator":"raytrace","settings":{"width":8,"height":8},"aging":[layer],
        "raytrace":{},"environment":{"path":"must-not-open-aging.hdr"}});
    let mut duplicate = base.clone();
    duplicate["raytrace"]["aging"] = json!([layer]);
    let mut anisotropic = base.clone();
    anisotropic["raytrace"]["anisotropic_faces"] =
        json!([{"body_index":0,"body_name":"Panel","face":0,"tangent":[1,0,0],"orientation":"zero","roughness_x":0.2,"roughness_y":0.6},null]);
    let mut raster = base;
    raster["integrator"] = json!("raster");
    for (input, reason) in [(duplicate, "not both"), (anisotropic, "anisotropic roughness"), (raster, "CPU raytrace integrator")] {
        for command in ["render.preflight", "render.start"] {
            let error = s.execute(command, &input).unwrap_err().to_string();
            assert!(error.contains(reason), "{error}");
            assert!(!error.contains("must-not-open-aging.hdr"));
            assert!(Arc::ptr_eq(&document, &s.doc));
            assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
            assert_eq!(host.admitted_bytes(), 0);
        }
    }
}

#[cfg(all(feature = "bundled-render-library", feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_saved_surface_preflight_and_real_factory_render_captured_builtins_after_reopen_without_mutation() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Coupon"})).unwrap();
    let scene = crate::product_scene::extract(&s).unwrap();
    let mut camera = solidcraft_render::Camera::default();
    camera.fit(&scene.bounds);
    s.execute("render.library.surface.bind",&json!({"name":"CPU maps","surface":{"id":"SURF-001","body_index":0,"body_name":"Coupon"},
        "set":{"camera":camera,"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":3,"seed":9},"output":{"linear_exr":true}}}}})).unwrap();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.execute("render.scene", &json!({})).unwrap();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let state = s.model.state();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &json!({"setup":"CPU maps"})).unwrap();
    assert_eq!(checked["textures"]["bodies"], 1);
    assert_eq!(checked["resolved"]["executor"], "cpu-raytrace");
    assert_eq!(host.admitted_bytes(), 0);
    let started = s.execute("render.start", &json!({"setup":"CPU maps"})).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(job).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
    assert!(status.request.textures.is_none());
    assert_eq!(status.textures.unwrap().bodies, 1);
    let frame = handle.frame().unwrap().unwrap();
    assert!(frame.linear_exr().is_some());
    assert_eq!(image::load_from_memory(frame.png()).unwrap().to_rgba8().dimensions(), (8, 8));
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
    assert!(Arc::ptr_eq(&state, &s.model.state()));
    let mut plain = s.doc.render.setups["CPU maps"].clone();
    plain.surfaces = Default::default();
    let started = s.execute("render.start", &json!({"settings":plain,"replace":true})).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let baseline = s.render_job_handle(job).unwrap();
    assert_eq!(baseline.wait_timeout(std::time::Duration::from_secs(60)).unwrap().progress.phase, solidcraft_render::job::Phase::Completed);
    assert_ne!(
        frame.png(),
        baseline.frame().unwrap().unwrap().png(),
        "the engine must capture and consume actual builtin maps, not only publish metadata"
    );
    assert!(Arc::ptr_eq(&document, &s.doc));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_adaptive_saved_and_runtime_admission_include_pilots_before_hdr_or_geometry_io() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for (width, height, samples) in [(8, 8, 64), (1024, 1024, 16)] {
        let request = json!({"integrator":"raytrace","settings":{"width":width,"height":height},
            "raytrace":{"progressive":{"samples_per_pixel":samples,"seed":81},"adaptive":{"pilot_samples":16,"min_samples":16}},
            "environment":{"path":"must-not-open-adaptive.hdr"}});
        for command in ["render.preflight", "render.start"] {
            let error = s.execute(command, &request).unwrap_err().to_string();
            assert!(error.contains("CPU") || error.contains("camera-sample"), "{error}");
            assert!(!error.contains("must-not-open-adaptive.hdr"));
            assert_eq!(host.admitted_bytes(), 0);
            assert!(Arc::ptr_eq(&document, &s.doc));
            assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
        }
    }
    s.execute(
        "render.settings.set",
        &json!({"name":"Adaptive CPU","set":{"width":3,"height":1,
        "job":{"method":"cpu_raytrace","transparent_background":true,"options":{"progressive":{"samples_per_pixel":48,"seed":81},
            "adaptive":{"pilot_samples":16,"min_samples":16},"lights":[],"denoise":true,"output":{"linear_exr":true}}}}}),
    )
    .unwrap();
    let saved = s.doc.render.setups["Adaptive CPU"].clone();
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["Adaptive CPU"], saved);
    s = Session::new(reopened);
    s.set_render_job_host(Some(host)).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let doc = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &json!({"setup":"Adaptive CPU"})).unwrap();
    assert_eq!(
        checked["request"]["raytrace"]["adaptive"],
        json!({"pilot_samples":16,"min_samples":16,"relative_error":0.05,"absolute_error":0.002,"coverage_error":0.01})
    );
    let id: JobId = serde_json::from_value(s.execute("render.start", &json!({"setup":"Adaptive CPU"})).unwrap()["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
    assert_eq!(json!(status.request), checked["request"]);
    let report = status.sampling.unwrap();
    assert_eq!((report.pilot_samples_per_pixel, report.production_samples, report.primary_path_evaluations), (16, 48, 96));
    assert!(handle.frame().unwrap().unwrap().linear_exr().is_some());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_general_closed_mesh_path_matches_runtime_and_actual_captured_cylinder_after_reopen() {
    use solidcraft_render::job::{ImageRequest, Phase};
    let mut s = Session::default();
    s.execute("solid.cylinder", &json!({"radius":5,"height":3,"body_name":"Curved"})).unwrap();
    let camera = solidcraft_render::Camera {
        target: solidcore_record::Vec3::new(0.0, 0.0, 1.5),
        distance: 20.0,
        yaw: 0.3,
        pitch: 0.4,
        fov: 0.5,
        ..Default::default()
    };
    s.execute(
        "render.settings.set",
        &json!({"name":"General Glass","set":{"camera":camera,"width":3,"height":3,
        "job":{"method":"cpu_raytrace","options":{"path":{"max_depth":12},"progressive":{"samples_per_pixel":3,"seed":81},
        "lights":[],"environment":{"color":[1,1,1],"intensity":1},"output":{"linear_exr":true}}}}}),
    )
    .unwrap();
    let geometry = s.doc.features.clone();
    let materials = s.doc.materials.clone();
    let undo = s.undo.len();
    s.execute(
        "render.optics.bind",
        &json!({"name":"General Glass","body_index":0,"body_name":"Curved","expected_revision":s.revision,
        "optics":{"ior":1.51,"absorption_per_mm":[0.1,0.02,0],"boundary":"closed_mesh"}}),
    )
    .unwrap();
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(s.doc.features, geometry);
    assert_eq!(s.doc.materials, materials);
    let saved = s.doc.to_json();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc.render.setups["General Glass"].optics.is_empty());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.to_json(), saved);
    s = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host)).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &json!({"setup":"General Glass"})).unwrap();
    assert_eq!(checked["dielectric_boundaries"], json!(["thin_sheet", "closed_box", "closed_mesh"]));
    assert_eq!(checked["stage_transport"], "CPU finite-depth diffuse/specular and dielectric path lighting");
    let captured: ImageRequest = serde_json::from_value(checked["request"].clone()).unwrap();
    assert_eq!(captured.raytrace.as_ref().unwrap().dielectrics[0].boundary, solidcraft_render::job::DielectricBoundary::ClosedMesh);
    assert_eq!(captured.raytrace.as_ref().unwrap().path.unwrap().max_depth, 12);
    let runtime = s
        .execute("render.preflight", &json!({"integrator":"raytrace","settings":{"camera":camera,"width":3,"height":3},"raytrace":captured.raytrace}))
        .unwrap();
    assert_eq!(runtime["request"], checked["request"]);
    let started = s.execute("render.start", &json!({"setup":"General Glass"})).unwrap();
    let id = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let complete = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(complete.progress.phase, Phase::Completed, "{:?}", complete.error);
    assert_eq!(json!(complete.request), checked["request"]);
    let frame = handle.frame().unwrap().unwrap();
    assert!(!frame.png().is_empty());
    assert!(frame.linear_exr().is_some());
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), before);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn runtime_checked_hdr_keeps_two_stable_glass_targets_and_exact_cpu_options_without_saving_the_draft() {
    use solidcraft_doc::render_optics::{OpticalMaterial, SavedOptics};
    use solidcraft_doc::render_settings::RenderJob;
    use solidcraft_render::job::Phase;
    let mut s = Session::default();
    for (name, corner) in [("Saved glass", [0, 0, 0]), ("Draft glass", [4, 0, 0])] {
        s.execute("solid.box", &json!({"body_name":name,"length":2,"width":2,"height":2,"corner":corner,"operation":"new"})).unwrap();
    }
    let camera = solidcraft_render::Camera { target: solidcore_record::Vec3::new(3.0, 1.0, 1.0), distance: 20.0, fov: 0.6, ..Default::default() };
    s.execute("render.settings.set", &json!({"name":"Glass HDR","set":{"width":4,"height":3,"camera":camera,"exposure_ev":0.75,
        "job":{"method":"cpu_raytrace","transparent_background":true,"options":{"path":{"max_depth":12},"progressive":{"samples_per_pixel":3,"seed":81},
        "lights":[],"lens":{"aperture_radius_mm":0.01,"focus_distance_mm":20},"output":{"linear_exr":true}}}}})).unwrap();
    let targets = s.execute("render.library.targets", &json!({})).unwrap();
    let target = |name: &str| targets["items"].as_array().unwrap().iter().find(|row| row["name"] == name).unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"Glass HDR","body_index":target("Saved glass")["ordinal"],"body_name":"Saved glass",
        "expected_revision":s.revision,"optics":{"ior":1.5,"boundary":"closed_mesh","absorption_per_mm":[0.1,0.0,0.0]}}),
    )
    .unwrap();
    let saved = s.doc.render.setups["Glass HDR"].clone();
    let mut draft = saved.clone();
    draft
        .optics
        .set(
            SavedOptics {
                target: serde_json::from_value(target("Draft glass")["stable_target"].clone()).unwrap(),
                optics: OpticalMaterial { ior: 1.2, absorption_per_mm: [0.0, 0.2, 0.0], ..Default::default() },
            },
            false,
        )
        .unwrap();
    let Some(RenderJob::CpuRaytrace { options, .. }) = draft.job.take() else { panic!("CPU fixture") };
    let options = options.to_runtime(draft.exposure_ev).unwrap();
    draft.exposure_ev = 0.0;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let path = std::env::temp_dir().join(format!("solidcraft-stable-glass-hdr-{}-{nonce}.hdr", std::process::id()));
    let mut bytes = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\nPRIMARIES=0.64 0.33 0.30 0.60 0.15 0.06 0.3127 0.3290\n\n-Y 2 +X 4\n".to_vec();
    for _ in 0..8 {
        bytes.extend_from_slice(&[128, 128, 128, 129]);
    }
    std::fs::write(&path, &bytes).unwrap();
    let info = s.execute("render.hdr.inspect", &json!({"path":path})).unwrap();
    let params = json!({"integrator":"raytrace","settings":draft,"raytrace":options,"transparent_background":true,
        "environment":{"path":path,"expected_sha256":info["sha256"],"rotation_radians":0.75,"intensity":1.25,"samples":4}});
    let original = params.clone();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let model = s.model.state();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &params).unwrap();
    let mut captured: solidcraft_render::job::ImageRequest = serde_json::from_value(checked["request"].clone()).unwrap();
    let optics = &captured.raytrace.as_ref().unwrap().dielectrics;
    assert_eq!(optics.len(), 2);
    for (name, ior, absorption) in [("Saved glass", 1.5, [0.1, 0.0, 0.0]), ("Draft glass", 1.2, [0.0, 0.2, 0.0])] {
        let actual = optics.iter().find(|entry| entry.body_name == name).unwrap();
        assert_eq!(actual.body_index, target(name)["ordinal"].as_u64().unwrap() as usize);
        assert_eq!((actual.ior, actual.absorption_per_mm), (ior, absorption));
        assert_eq!(actual.boundary, solidcraft_render::job::DielectricBoundary::ClosedMesh);
    }
    captured.raytrace.as_mut().unwrap().dielectrics.clear();
    assert_eq!(captured.raytrace.as_ref().unwrap(), &options);
    assert_eq!(captured.camera, camera);
    assert!(captured.transparent_background);
    assert_eq!(checked["environment"]["rotation_radians"], 0.75);
    assert_eq!(checked["environment"]["intensity"], 1.25);
    let mut mixed = params.clone();
    mixed["raytrace"]["dielectrics"] = checked["request"]["raytrace"]["dielectrics"].clone();
    mixed["environment"]["path"] = json!("must-not-open-mixed-stable-glass.hdr");
    assert!(s.execute("render.preflight", &mixed).unwrap_err().to_string().contains("cannot combine with runtime"));
    let mut raster = params.clone();
    raster["integrator"] = json!("raster");
    raster.as_object_mut().unwrap().remove("raytrace");
    raster["environment"]["path"] = json!("must-not-open-raster-stable-glass.hdr");
    assert!(s.execute("render.preflight", &raster).unwrap_err().to_string().contains("stable optics require"));
    let mut over_cap = params.clone();
    over_cap["settings"]["optics"] = json!(
        (0..9)
            .map(|i| {
                let mut row = params["settings"]["optics"][0].clone();
                row["target"]["definition"] = json!(format!("Unique unknown optical body {i}"));
                row
            })
            .collect::<Vec<_>>()
    );
    over_cap["environment"]["path"] = json!("must-not-open-over-cap-stable-glass.hdr");
    assert!(s.execute("render.preflight", &over_cap).unwrap_err().to_string().contains("at most eight"));
    let mut changed_hdr = params.clone();
    changed_hdr["environment"]["expected_sha256"] = json!("0".repeat(64));
    assert!(s.execute("render.preflight", &changed_hdr).unwrap_err().to_string().contains("changed since selection"));
    assert_eq!(host.admitted_bytes(), 0);
    let started = s.execute("render.start", &params).unwrap();
    assert_eq!(started["request"], checked["request"]);
    std::fs::remove_file(path).unwrap();
    let id = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = s.render_job_handle(id).unwrap();
    let terminal = handle.wait_timeout(std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(terminal.progress.phase, Phase::Completed, "{:?}", terminal.error);
    assert_eq!(json!(terminal.request), checked["request"]);
    let frame = handle.frame().unwrap().unwrap();
    assert!(!frame.png().is_empty() && frame.linear_exr().is_some());
    assert!(Arc::ptr_eq(&document, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(history, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
    assert_eq!(s.doc.render.setups["Glass HDR"], saved);
    assert_eq!(params, original);
}

#[cfg(all(feature = "bundled-render-library", feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_path_maps_glass_and_targeted_aging_reach_real_preflight_and_render_after_cold_reopen() {
    let mut s = Session::default();
    for (name, corner) in [("Glass", [0, 0, 0]), ("Panel", [20, 0, 0])] {
        s.execute("solid.box", &json!({"body_name":name,"length":10,"width":10,"height":2,"corner":corner,"operation":"new"})).unwrap();
    }
    let targets = s.execute("render.library.targets", &json!({})).unwrap();
    let glass = targets["items"].as_array().unwrap().iter().find(|target| target["name"] == "Glass").unwrap().clone();
    let panel = targets["items"].as_array().unwrap().iter().find(|target| target["name"] == "Panel").unwrap().clone();
    let scene = crate::product_scene::extract(&s).unwrap();
    let mut camera = solidcraft_render::Camera::default();
    camera.set_view(solidcraft_render::StandardView::Top);
    camera.fit(&scene.bounds);
    s.execute(
        "render.settings.set",
        &json!({"name":"Mixed","set":{"camera":camera,"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{
            "progressive":{"samples_per_pixel":3,"seed":9},"path":{"max_depth":4},"lights":[],
            "environment":{"color":[1,1,1],"intensity":1},"output":{"linear_exr":true},
            "aging":[{"kind":"fading","target":panel["stable_target"],"strength":0.25,"coverage":1.0,"upward_bias":0.0}]
        }}}}),
    )
    .unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"Mixed","body_index":glass["ordinal"],"body_name":"Glass","expected_revision":s.revision,
        "optics":{"ior":1.0,"absorption_per_mm":[0.1,0.2,0.0],"boundary":"closed_mesh"}}),
    )
    .unwrap();
    s.execute("render.library.surface.bind", &json!({"name":"Mixed","surface":{"id":"SURF-001","body_index":panel["ordinal"],"body_name":"Panel"}}))
        .unwrap();
    let setup = s.doc.render.setups["Mixed"].clone();
    s = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    s.execute("render.scene", &json!({})).unwrap();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let model = s.model.state();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &json!({"setup":"Mixed"})).unwrap();
    assert_eq!(checked["textures"]["bodies"], 1);
    assert_eq!(checked["request"]["raytrace"]["path"]["max_depth"], 4);
    assert_eq!(checked["request"]["raytrace"]["dielectrics"][0]["body_name"], "Glass");
    assert_eq!(checked["request"]["raytrace"]["dielectrics"][0]["boundary"], "closed_mesh");
    assert_eq!(checked["request"]["raytrace"]["aging"][0]["target"]["body_name"], "Panel");
    let run = |session: &mut Session, params: Value| {
        let started = session.execute("render.start", &params).unwrap();
        let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
        let handle = session.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
        let started_cleanup = std::time::Instant::now();
        while !handle.work_released() && started_cleanup.elapsed() < std::time::Duration::from_secs(60) {
            std::thread::yield_now();
        }
        assert!(handle.work_released(), "pool assertions require completed worker cleanup, not only a terminal phase");
        let frame = handle.frame().unwrap().unwrap();
        assert_eq!(image::load_from_memory(frame.png()).unwrap().to_rgba8().dimensions(), (8, 8));
        assert!(frame.linear_exr().is_some());
        (frame.png().to_vec(), frame.linear_exr().unwrap().to_vec())
    };
    let saved = run(&mut s, json!({"setup":"Mixed"}));
    assert_eq!(saved, run(&mut s, json!({"settings":setup,"replace":true})), "named and prospective captures keep identical maps/optics/aging");
    let mut plain = setup.clone();
    plain.surfaces = Default::default();
    assert_ne!(
        saved,
        run(&mut s, json!({"settings":plain,"replace":true})),
        "the factory must consume actual opaque maps alongside Glass, not publish metadata alone"
    );
    for same_target in [true, false] {
        let mut invalid = setup.clone();
        if same_target {
            let mut binding = invalid.surfaces.iter().next().unwrap().clone();
            binding.target = invalid.optics.iter().next().unwrap().target.clone();
            invalid.surfaces = Default::default();
            invalid.surfaces.set(binding, false).unwrap();
        } else if let Some(solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = &mut invalid.job {
            options.path = None;
        }
        let bytes = host.admitted_bytes();
        assert!(s.execute("render.preflight", &json!({"settings":invalid})).is_err());
        assert_eq!(host.admitted_bytes(), bytes, "invalid authored combinations acquire no new render lease");
    }
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert!(Arc::ptr_eq(&model, &s.model.state()));
    assert_eq!(history, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_coat_cpu_direct_and_path_render_match_independent_normal_incidence_layer_and_zero_is_exact() {
    // Independent OpenEXR scanline/FLOAT reader for this fully covered1×1 oracle.
    // No production encoder/decoder or optional image codec participates.
    fn raw_pixel(bytes: &[u8]) -> [f32; 4] {
        fn word(bytes: &[u8], at: usize) -> u32 {
            u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
        }
        fn string<'a>(bytes: &'a [u8], at: &mut usize) -> &'a [u8] {
            let start = *at;
            *at += bytes[start..].iter().position(|b| *b == 0).unwrap() + 1;
            &bytes[start..*at - 1]
        }
        assert_eq!(word(bytes, 0), 20_000_630);
        assert_eq!(word(bytes, 4), 2);
        let mut at = 8;
        let (mut channels, mut window, mut compression) = (false, false, false);
        while bytes[at] != 0 {
            let name = string(bytes, &mut at);
            let kind = string(bytes, &mut at);
            let length = word(bytes, at) as usize;
            at += 4;
            let data = &bytes[at..at + length];
            at += length;
            match name {
                b"channels" => {
                    assert_eq!(kind, b"chlist");
                    let mut c = 0;
                    for expected in [b"A", b"B", b"G", b"R"] {
                        assert_eq!(string(data, &mut c), expected);
                        assert_eq!(word(data, c), 2); // FLOAT, never HALF
                        assert_eq!(&data[c + 4..c + 8], &[0; 4]);
                        assert_eq!((word(data, c + 8), word(data, c + 12)), (1, 1));
                        c += 16;
                    }
                    assert_eq!(&data[c..], &[0]);
                    channels = true;
                }
                b"dataWindow" => {
                    assert_eq!(kind, b"box2i");
                    assert_eq!(data, &[0; 16]);
                    window = true;
                }
                b"compression" => {
                    assert_eq!(kind, b"compression");
                    assert_eq!(data, &[0]);
                    compression = true;
                }
                _ => {}
            }
        }
        assert!(channels && window && compression);
        at += 1;
        let offset = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
        assert_eq!((word(bytes, offset), word(bytes, offset + 4)), (0, 16));
        let channel = |i: usize| f32::from_bits(word(bytes, offset + 8 + i * 4));
        [channel(3), channel(2), channel(1), channel(0)]
    }
    use solidcore_record::Vec3;
    use solidcraft_doc::render_coat::SavedCoat;
    use std::f64::consts::{FRAC_PI_2, PI, SQRT_2};
    for indirect in [false, true] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"body_name":"Panel","corner":[-10,-10,-2],"length":20,"width":20,"height":2})).unwrap();
        s.execute(
            "appearance.assign",
            &json!({"bodies":["Panel"],"name":"Analytic substrate","color":[128,64,32],
            "pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1}}),
        )
        .unwrap();
        let camera =
            solidcraft_render::Camera { target: Vec3::ZERO, distance: 1.0, yaw: 0.0, pitch: FRAC_PI_2, roll: 0.0, fov: 0.0, ..Default::default() };
        let mut options = json!({"progressive":{"samples_per_pixel":4,"seed":81},"lights":[{"direction":[0,0,1],"color":[1,1,1],"intensity":PI}],
            "environment":{"color":[0,0,0],"intensity":0},"output":{"linear_exr":true}});
        if indirect {
            options["path"] = json!({"max_depth":1});
        }
        s.execute("render.coat.bind", &json!({"name":"Coat","body_index":0,"body_name":"Panel","expected_revision":s.revision,
            "coat":{"weight":0.63,"roughness":0.61,"ior":1.7},"set":{"camera":camera,"width":1,"height":1,"job":{"method":"cpu_raytrace","options":options}}})).unwrap();
        let container = solidcraft_io::write_design_container(&s.doc).unwrap();
        s = Session::new(solidcraft_io::read_design(&container).unwrap());
        s.execute("render.scene", &json!({})).unwrap();
        let host = CpuHost::for_platform().unwrap();
        s.set_render_job_host(Some(host.clone())).unwrap();
        let document = s.doc.clone();
        let geometry = s.model.state();
        let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
        let checked = s.execute("render.preflight", &json!({"setup":"Coat"})).unwrap();
        assert_eq!(checked["coats"], json!(s.doc.render.setups["Coat"].coats));
        assert_eq!(checked["resolved"]["executor"], "cpu-raytrace");
        assert_eq!(host.admitted_bytes(), 0);
        let complete = |s: &mut Session, settings: Value| {
            let reply = s.execute("render.start", &json!({"settings":settings})).unwrap();
            let job: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
            let handle = s.render_job_handle(job).unwrap();
            let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
            assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
            handle.frame().unwrap().unwrap()
        };
        let setup = s.doc.render.setups["Coat"].clone();
        let frame = complete(&mut s, json!(setup));
        let raw = raw_pixel(frame.linear_exr().unwrap());
        assert_eq!(raw[3], 1.0);
        // Closed-form normal-incidence coat over an IOR1 Lambert substrate.
        // The model's conservative entry/exit factor uses its45-degree bound.
        let f0 = ((1.7_f64 - 1.0) / (1.7 + 1.0)).powi(2);
        let bound = f0 + (1.0 - f0) * (1.0 - 1.0 / SQRT_2).powi(5);
        let attenuation = (1.0 - 0.63 * bound).powi(2);
        let highlight = 0.63 * f0 / (4.0 * 0.61_f64.powi(4));
        let expected = [128_u8, 64, 32].map(|code| {
            let encoded = f64::from(code) / 255.0;
            let linear = if encoded <= 0.04045 { encoded / 12.92 } else { ((encoded + 0.055) / 1.055).powf(2.4) };
            linear * attenuation + highlight
        });
        for (actual, expected) in raw[..3].iter().zip(expected) {
            assert!((f64::from(*actual) - expected).abs() < 2e-7, "direct/path coat radiance {actual} vs independent {expected}");
        }
        let mut plain = setup.clone();
        plain.coats = Default::default();
        let plain_check = s.execute("render.preflight", &json!({"settings":plain})).unwrap();
        assert_eq!(checked["admission"], plain_check["admission"], "coat uses existing inline surface metadata and the same bounded producer stages");
        assert_eq!(checked["retained_request_bytes"], plain_check["retained_request_bytes"]);
        let mut zero = setup;
        let row = zero.coats.iter().next().unwrap().clone();
        zero.coats.set(SavedCoat { coat: solidcraft_doc::render_coat::CoatMaterial { weight: 0.0, ..row.coat }, ..row }, true).unwrap();
        let baseline = complete(&mut s, json!(plain));
        let identity = complete(&mut s, json!(zero));
        assert_eq!(baseline.png(), identity.png());
        assert_eq!(baseline.linear_exr().unwrap(), identity.linear_exr().unwrap());
        assert_ne!(frame.linear_exr().unwrap(), baseline.linear_exr().unwrap(), "active coat must change actual transport, not metadata only");
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert!(Arc::ptr_eq(&geometry, &s.model.state()));
        assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), before);
    }
}

#[cfg(all(feature = "bundled-render-library", feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_coat_survives_builtin_maps_aging_and_guided_filter_without_changing_the_raw_master() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"body_name":"Panel","corner":[-10,-10,-2],"length":20,"width":20,"height":2})).unwrap();
    s.execute("appearance.assign", &json!({"bodies":["Panel"],"color":[128,64,32],"pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1}})).unwrap();
    let camera =
        solidcraft_render::Camera { distance: 10.0, yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, roll: 0.0, fov: 0.0, ..Default::default() };
    s.execute(
        "render.coat.bind",
        &json!({"name":"Coat","body_index":0,"body_name":"Panel","expected_revision":s.revision,
        "coat":{"weight":0.63,"roughness":0.61,"ior":1.7},"set":{"camera":camera,"width":16,"height":16,"job":{"method":"cpu_raytrace","options":{
            "progressive":{"samples_per_pixel":4,"seed":81},"path":{"max_depth":2},"shadows":true,"denoise":true,
            "lights":[{"direction":[0,0,1],"color":[1,1,1],"intensity":3}],"environment":{"color":[0,0,0],"intensity":0},
            "aging":[{"kind":"dust","strength":0.2,"coverage":1}],"output":{"linear_exr":true}}}}}),
    )
    .unwrap();
    s.execute("render.library.surface.bind", &json!({"name":"Coat","surface":{"id":"SURF-001","body_index":0,"body_name":"Panel","scale":10}}))
        .unwrap();
    let bytes = solidcraft_io::write_design_container(&s.doc).unwrap();
    s = Session::new(solidcraft_io::read_design(&bytes).unwrap());
    s.execute("render.scene", &json!({})).unwrap();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host)).unwrap();
    let document = s.doc.clone();
    let state = s.model.state();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let complete = |s: &mut Session, setup: solidcraft_doc::render_settings::RenderSetup| {
        let reply = s.execute("render.start", &json!({"settings":setup})).unwrap();
        let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
        let handle = s.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, solidcraft_render::job::Phase::Completed, "{:?}", status.error);
        handle.frame().unwrap().unwrap()
    };
    let saved = s.doc.render.setups["Coat"].clone();
    let actual = complete(&mut s, saved.clone());
    let mut uncoated = saved.clone();
    uncoated.coats = Default::default();
    let plain = complete(&mut s, uncoated);
    assert_ne!(actual.linear_exr().unwrap(), plain.linear_exr().unwrap(), "maps and aging must not erase the captured coat");
    let mut unmapped = saved.clone();
    unmapped.surfaces = Default::default();
    let no_map = complete(&mut s, unmapped);
    assert_ne!(actual.linear_exr().unwrap(), no_map.linear_exr().unwrap(), "saved maps must still reach the coated closure");
    let mut unaged = saved.clone();
    let Some(solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = &mut unaged.job else { panic!("CPU") };
    options.aging = Default::default();
    let no_age = complete(&mut s, unaged);
    assert_ne!(actual.linear_exr().unwrap(), no_age.linear_exr().unwrap(), "aging still modifies the mapped coat/substrate");
    let mut unfiltered = saved.clone();
    let Some(solidcraft_doc::render_settings::RenderJob::CpuRaytrace { options, .. }) = &mut unfiltered.job else { panic!("CPU") };
    options.denoise = false;
    let raw = complete(&mut s, unfiltered);
    assert_eq!(actual.linear_exr().unwrap(), raw.linear_exr().unwrap(), "guided finishing cannot modify coated raw transport");
    assert_eq!(s.doc.render.setups["Coat"], saved);
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert!(Arc::ptr_eq(&state, &s.model.state()));
    assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), history);
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_subsurface_cold_capture_matches_runtime_real_png_raw_and_one_undo_without_geometry_changes() {
    use solidcraft_render::job::{ImageRequest, Phase};
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Scattering"})).unwrap();
    let camera = solidcraft_render::Camera {
        target: solidcore_record::Vec3::new(5.0, 5.0, 0.5),
        distance: 0.5,
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        fov: 0.6,
        ..Default::default()
    };
    s.execute(
        "render.settings.set",
        &json!({"name":"SSS","set":{"camera":camera,"width":3,"height":3,"job":{"method":"cpu_raytrace","options":{
            "path":{"max_depth":16},"progressive":{"samples_per_pixel":3,"seed":9},"lights":[],
            "environment":{"color":[1,1,1],"intensity":1},"output":{"linear_exr":true}
        }}}}),
    )
    .unwrap();
    let before = s.doc.clone();
    let undo = s.undo.len();
    s.execute(
        "render.optics.bind",
        &json!({"name":"SSS","body_index":0,"body_name":"Scattering","expected_revision":s.revision,
        "optics":{"ior":1.0,"boundary":"closed_mesh","absorption_per_mm":[0.1,0.2,0.0],"subsurface":{"scattering_per_mm":0.3,"anisotropy":0.4}}}),
    )
    .unwrap();
    let saved = s.doc.clone();
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(saved.features, before.features);
    assert_eq!(saved.materials, before.materials);
    assert_eq!(saved.appearances, before.appearances);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc, saved);
    s = Session::new(solidcraft_io::read_design(&solidcraft_io::write_design_container(&saved).unwrap()).unwrap());
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let document = s.doc.clone();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &json!({"setup":"SSS"})).unwrap();
    let captured: ImageRequest = serde_json::from_value(checked["request"].clone()).unwrap();
    let input = &captured.raytrace.as_ref().unwrap().dielectrics[0];
    assert_eq!(input.subsurface.unwrap().scattering_per_mm, 0.3);
    assert_eq!(input.subsurface.unwrap().anisotropy, 0.4);
    let runtime = json!({"integrator":"raytrace","settings":{"camera":camera,"width":3,"height":3},"raytrace":captured.raytrace});
    assert_eq!(s.execute("render.preflight", &runtime).unwrap()["request"], checked["request"]);
    let run = |s: &mut Session, params: Value| {
        let started = s.execute("render.start", &params).unwrap();
        let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
        let handle = s.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        let frame = handle.frame().unwrap().unwrap();
        (frame.png().to_vec(), frame.linear_exr().unwrap().to_vec())
    };
    let pixels = run(&mut s, json!({"setup":"SSS"}));
    let mut runtime = runtime;
    runtime["replace"] = json!(true);
    assert_eq!(pixels, run(&mut s, runtime));
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(history, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn saved_cpu_atmosphere_cold_capture_start_matches_runtime_pixels_raw_and_one_undo_without_camera_or_geometry_edits() {
    use solidcraft_render::job::{ImageRequest, Phase};
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":2,"body_name":"Fog receiver"})).unwrap();
    let camera = solidcraft_render::Camera {
        target: solidcore_record::Vec3::new(5.0, 5.0, 1.0),
        distance: 5.0,
        yaw: 0.0,
        pitch: std::f64::consts::FRAC_PI_2,
        fov: 0.5,
        ..Default::default()
    };
    s.execute("render.settings.set",&json!({"name":"Fog","set":{"camera":camera,"width":8,"height":8,"job":{"method":"cpu_raytrace","options":{
        "path":{"max_depth":8},"progressive":{"samples_per_pixel":3,"seed":9},"lights":[],"environment":{"color":[1,1,1],"intensity":1},"output":{"linear_exr":true}
    }}}})).unwrap();
    let before = s.doc.clone();
    let undo = s.undo.len();
    let volume = json!({"min_mm":[-10,-10,-1],"max_mm":[20,20,5],"extinction_per_mm":0.3,"scattering_albedo":[0.2,0.4,0.6],"anisotropy":-0.37});
    let mut job = serde_json::to_value(before.render.setups["Fog"].job.as_ref().unwrap()).unwrap();
    job["options"]["volume"] = volume.clone();
    s.execute("render.settings.set", &json!({"name":"Fog","set":{"job":job}})).unwrap();
    let saved = s.doc.clone();
    assert_eq!(s.undo.len(), undo + 1);
    assert_eq!(saved.render.setups["Fog"].camera, before.render.setups["Fog"].camera);
    assert_eq!(saved.features, before.features);
    assert_eq!(saved.materials, before.materials);
    assert_eq!(saved.appearances, before.appearances);
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc, saved);
    s = Session::new(solidcraft_io::read_design(&solidcraft_io::write_design_container(&saved).unwrap()).unwrap());
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("render.scene", &json!({})).unwrap();
    let document = s.doc.clone();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let checked = s.execute("render.preflight", &json!({"setup":"Fog"})).unwrap();
    let request: ImageRequest = serde_json::from_value(checked["request"].clone()).unwrap();
    let authored: solidcraft_render::job::volume::HomogeneousVolume = serde_json::from_value(volume).unwrap();
    assert_eq!(request.raytrace.as_ref().unwrap().volume, Some(authored));
    let runtime = json!({"integrator":"raytrace","settings":{"camera":camera,"width":8,"height":8},"raytrace":request.raytrace});
    assert_eq!(s.execute("render.preflight", &runtime).unwrap()["request"], checked["request"]);
    let run = |s: &mut Session, params: Value| {
        let started = s.execute("render.start", &params).unwrap();
        let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
        let handle = s.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        let frame = handle.frame().unwrap().unwrap();
        (frame.png().to_vec(), frame.linear_exr().unwrap().to_vec())
    };
    let pixels = run(&mut s, json!({"setup":"Fog"}));
    let mut runtime = runtime;
    runtime["replace"] = json!(true);
    assert_eq!(pixels, run(&mut s, runtime));
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(history, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}

#[test]
fn saved_cpu_atmosphere_incompatibility_is_pure_before_hdr_capture_and_keeps_authored_data() {
    let mut s = Session::default();
    let before = s.doc.clone();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let volume = json!({"min_mm":[-2,-2,-2],"max_mm":[2,2,2],"extinction_per_mm":0.3,"scattering_albedo":[0.2,0.4,0.6],"anisotropy":-0.37});
    for mut options in
        [json!({"volume":volume,"denoise":true,"path":{"max_depth":4},"progressive":{"samples_per_pixel":4,"seed":9}}), json!({"volume":volume})]
    {
        options["hdr"] = json!({"resource":"0".repeat(64),"rotation_rad":0,"intensity":1,"samples":16});
        let params = json!({"settings":{"camera":solidcraft_render::Camera::default(),"width":4,"height":4,"job":{"method":"cpu_raytrace","options":options}}});
        let error = s.execute("render.preflight", &params).unwrap_err().to_string();
        assert!(error.contains("Atmosphere"), "{error}");
        assert!(Arc::ptr_eq(&before, &s.doc));
        assert_eq!(history, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
    }
}

#[test]
fn saved_cpu_atmosphere_and_glass_refuse_before_missing_hdr_access_without_dropping_either_authored_choice() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length":4,"width":4,"height":2,"body_name":"Glass"})).unwrap();
    s.execute("render.settings.set",&json!({"name":"Fog","set":{"width":4,"height":4,"job":{"method":"cpu_raytrace","options":{"progressive":{"samples_per_pixel":4,"seed":9}}}}})).unwrap();
    s.execute("render.optics.bind",&json!({"name":"Fog","body_index":0,"body_name":"Glass","expected_revision":s.revision,"optics":{"ior":1.5,"boundary":"closed_box","shadow_transmission":false}})).unwrap();
    let mut settings = serde_json::to_value(&s.doc.render.setups["Fog"]).unwrap();
    settings["job"]["options"]["path"] = json!({"max_depth":4});
    settings["job"]["options"]["volume"] =
        json!({"min_mm":[-10,-10,-10],"max_mm":[10,10,10],"extinction_per_mm":0.2,"scattering_albedo":[0.1,0.3,0.7],"anisotropy":0.2});
    settings["job"]["options"]["hdr"] = json!({"resource":"0".repeat(64),"rotation_rad":0,"intensity":1,"samples":16});
    let authored = settings.clone();
    let document = s.doc.clone();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    for command in ["render.preflight", "render.start", "render.settings.set"] {
        let params = if command == "render.settings.set" { json!({"name":"Fog","set":settings}) } else { json!({"settings":settings}) };
        let error = s.execute(command, &params).unwrap_err().to_string();
        assert!(error.contains("Atmosphere"), "{command}: {error}");
        assert_eq!(settings, authored);
        assert!(Arc::ptr_eq(&document, &s.doc));
        assert_eq!(history, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_sun_sky_named_runtime_reopen_and_actual_output_keep_the_same_capture() {
    use solidcraft_render::job::Phase;
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    s.execute("solid.box", &json!({"length":10,"width":10,"height":3})).unwrap();
    let camera = solidcraft_render::Camera { distance: 30.0, fov: 0.7, ..Default::default() };
    let options = json!({"path":{"max_depth":3},"progressive":{"samples_per_pixel":3,"seed":81},"lights":[],"denoise":true,
        "sun":{"direction":[0.2,-0.3,0.9],"angular_radius_rad":0.2,"radiance":[10,8,6]},
        "sky":{"kind":"sky","gradient":{"zenith":[0.25,0.5,1.0],"horizon":[0.75,0.5,0.25],"ground":[0.125,0.25,0.5]},"rotation_rad":0.731,"intensity":1.75},
        "lens":{"aperture_radius_mm":0.01,"focus_distance_mm":30},"output":{"linear_exr":true}});
    s.execute("render.settings.set",&json!({"name":"CPU celestial","set":{"width":8,"height":6,"camera":camera,"job":{"method":"cpu_raytrace","transparent_background":true,"options":options}}})).unwrap();
    let saved = s.doc.render.setups["CPU celestial"].clone();
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.render.setups["CPU celestial"], saved);
    s = Session::new(reopened);
    s.set_render_job_host(Some(host.clone())).unwrap();
    let checked = s.execute("render.preflight", &json!({"setup":"CPU celestial"})).unwrap();
    let mut runtime_options = options.clone();
    runtime_options["sky"].as_object_mut().unwrap().remove("kind");
    let runtime =
        json!({"integrator":"raytrace","settings":{"width":8,"height":6,"camera":camera},"transparent_background":true,"raytrace":runtime_options});
    assert_eq!(checked["request"], s.execute("render.preflight", &runtime).unwrap()["request"]);
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let mut pngs = Vec::new();
    let mut masters = Vec::new();
    for input in [json!({"setup":"CPU celestial"}), runtime] {
        let id: JobId = serde_json::from_value(s.execute("render.start", &input).unwrap()["job"].clone()).unwrap();
        let handle = s.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        assert_eq!(json!(status.request), checked["request"]);
        let frame = handle.frame().unwrap().unwrap();
        pngs.push(frame.png().to_vec());
        masters.push(frame.linear_exr().unwrap().to_vec());
    }
    assert_eq!(pngs[0], pngs[1]);
    assert_eq!(masters[0], masters[1]);
    assert!(Arc::ptr_eq(&document, &s.doc));
    assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
}
#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn cpu_celestial_conflicts_refuse_before_hdr_vfs_and_never_mutate_the_scene() {
    let mut s = Session::default();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let before = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
    let sky = json!({"gradient":{"zenith":[1,1,1],"horizon":[1,1,1],"ground":[1,1,1]},"rotation_rad":0.731,"intensity":1});
    for path in [Value::Null, json!({"max_depth":3})] {
        let options = json!({"path":path,"progressive":{"samples_per_pixel":3,"seed":81},"sky":sky});
        let input =
            json!({"integrator":"raytrace","settings":{"width":8,"height":6},"raytrace":options,"environment":{"path":"must-not-read-cpu-sky.hdr"}});
        for command in ["render.preflight", "render.start"] {
            let error = s.execute(command, &input).unwrap_err().to_string();
            assert!(error.contains(if path.is_null() { "Indirect" } else { "choose either CPU HDR" }), "{error}");
            assert!(!error.contains("must-not-read-cpu-sky.hdr"));
            assert_eq!(host.admitted_bytes(), 0);
            assert!(Arc::ptr_eq(&document, &s.doc));
            assert_eq!(before, (s.revision, s.evaluations, s.undo.len(), s.redo.len()));
        }
    }
}

mod bounded_render_preparation {
    use super::*;
    use crate::render_preparation::{Kind, finish, prepare};
    use solidcraft_render::job::{CancelToken, CpuHost, JobId};

    fn camera_bits(camera: Option<solidcraft_render::Camera>) -> Option<(solidcraft_render::UpAxis, [u64; 8])> {
        camera.map(|camera| {
            (
                camera.up,
                [camera.target.x, camera.target.y, camera.target.z, camera.yaw, camera.pitch, camera.roll, camera.distance, camera.fov]
                    .map(f64::to_bits),
            )
        })
    }
    fn fixture() -> (Session, CpuHost) {
        let host = CpuHost::cooperative();
        let mut session = Session::default();
        session.set_render_job_host(Some(host.clone())).unwrap();
        session.execute("render.scene", &json!({})).unwrap();
        (session, host)
    }
    fn params() -> Value {
        json!({"settings":{"width":3,"height":2}})
    }
    fn drain(host: &CpuHost) {
        for _ in 0..10_000 {
            if !host.pump(128).unwrap() {
                return;
            }
        }
        panic!("prepared tiny raster did not finish");
    }

    #[test]
    fn check_and_start_commit_only_in_the_real_session_with_exact_existing_reply_and_source() {
        let (mut session, host) = fixture();
        let doc = session.doc.clone();
        let state = session.model.state();
        let before = (session.revision, session.evaluations, session.undo.len(), session.redo.len(), camera_bits(session.camera));
        let expected = session.execute("render.preflight", &params()).unwrap();
        assert_eq!(host.admitted_bytes(), 0);
        let checked = prepare(session.scratch(), Kind::Check, params(), CancelToken::default()).unwrap();
        assert_eq!(checked.kind(), Kind::Check);
        assert!(host.admitted_bytes() > 0, "a completed preparation owns its reserved captured resources");
        assert_eq!(finish(&mut session, checked).unwrap(), expected);
        assert_eq!(host.admitted_bytes(), 0);
        let prepared = prepare(session.scratch(), Kind::Start, params(), CancelToken::default()).unwrap();
        let held = host.admitted_bytes();
        let started = finish(&mut session, prepared).unwrap();
        let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
        assert_eq!(id.serial, 1, "Check and scratch preparation never mint or consume job identities");
        assert_eq!(started["request"], expected["request"]);
        assert_eq!(host.admitted_bytes(), held, "commit transfers the same leases without another admission");
        assert!(session.scratch().execute("render.status", &json!({"job":id})).is_err(), "the scratch registry never owns the final job");
        drain(&host);
        let handle = session.render_job_handle(id).unwrap();
        assert_eq!(handle.status().unwrap().progress.phase, solidcraft_render::job::Phase::Completed);
        let actual = handle.frame().unwrap().unwrap();
        let (mut control, control_host) = fixture();
        let control_id: JobId = serde_json::from_value(control.execute("render.start", &params()).unwrap()["job"].clone()).unwrap();
        drain(&control_host);
        assert_eq!(actual.png(), control.render_job_handle(control_id).unwrap().frame().unwrap().unwrap().png());
        assert!(Arc::ptr_eq(&doc, &session.doc));
        assert!(Arc::ptr_eq(&state, &session.model.state()));
        assert_eq!((session.revision, session.evaluations, session.undo.len(), session.redo.len(), camera_bits(session.camera)), before);
    }

    #[test]
    fn cancelled_or_unverified_source_host_and_owner_changes_never_publish_a_prepared_job() {
        for change in 0..5 {
            let (mut session, host) = fixture();
            let cancel = CancelToken::default();
            let prepared = prepare(session.scratch(), Kind::Start, params(), cancel.clone()).unwrap();
            match change {
                0 => cancel.cancel(),
                1 => session.revision += 1,
                2 => session.camera = Some(solidcraft_render::Camera { distance: 51.0, ..Default::default() }),
                3 => session.set_render_job_host(Some(CpuHost::cooperative())).unwrap(),
                4 => {
                    let (mut unrelated, _) = fixture();
                    assert!(finish(&mut unrelated, prepared).is_err());
                    assert_eq!(host.admitted_bytes(), 0);
                    continue;
                }
                _ => panic!("unexpected prepared-source rejection fixture"),
            }
            let doc = session.doc.clone();
            let before = (session.revision, session.evaluations, session.undo.len(), session.redo.len());
            assert!(finish(&mut session, prepared).is_err());
            assert_eq!(host.admitted_bytes(), 0);
            assert!(!host.pump(1).unwrap());
            assert!(Arc::ptr_eq(&doc, &session.doc));
            assert_eq!((session.revision, session.evaluations, session.undo.len(), session.redo.len()), before);
        }
    }

    #[test]
    fn preparation_checks_cancel_before_any_capture_or_admission() {
        let (session, host) = fixture();
        let cancel = CancelToken::default();
        cancel.cancel();
        assert!(prepare(session.scratch(), Kind::Check, params(), cancel).is_err());
        assert_eq!(host.admitted_bytes(), 0);
        assert!(!host.pump(1).unwrap());
    }
    #[test]
    fn prepared_capture_cannot_bypass_current_embedding_command_refusal() {
        let (mut session, host) = fixture();
        session.set_command_check(Some(Arc::new(|command, _, _| {
            if command == "render.preflight" { Err("embedding policy refuses checking".into()) } else { Ok(()) }
        })));
        let error = prepare(session.scratch(), Kind::Check, params(), solidcraft_render::job::CancelToken::default()).err().unwrap();
        assert!(error.to_string().contains("embedding policy refuses checking"));
        assert_eq!(host.admitted_bytes(), 0);
        session.set_command_check(None);
        let prepared = prepare(session.scratch(), Kind::Start, params(), solidcraft_render::job::CancelToken::default()).unwrap();
        session.set_command_check(Some(Arc::new(|command, _, _| {
            if command == "render.start" { Err("embedding policy changed before publication".into()) } else { Ok(()) }
        })));
        assert!(finish(&mut session, prepared).unwrap_err().to_string().contains("embedding policy changed"));
        assert_eq!(host.admitted_bytes(), 0);
        assert!(!host.pump(1).unwrap());
    }
    #[test]
    fn retained_completion_keeps_source_and_leases_until_same_worker_ack_on_every_outcome() {
        use crate::render_preparation::finish_retained;
        for outcome in ["check", "start", "stale", "host", "cancel", "queue"] {
            let (mut session, host) = fixture();
            let doc = session.doc.clone();
            let baseline_refs = Arc::strong_count(&doc);
            let cancel = solidcraft_render::job::CancelToken::default();
            let snapshot = session.scratch();
            let kind = if outcome == "check" { Kind::Check } else { Kind::Start };
            // The same bounded worker prepares, yields ownership, waits for it
            // back, disposes it, then acknowledges actual release.
            let (ready, receiver) = std::sync::mpsc::sync_channel(1);
            let (retire, returned) = std::sync::mpsc::sync_channel::<crate::render_preparation::Prepared>(1);
            let (ack, released) = std::sync::mpsc::sync_channel(1);
            let token = cancel.clone();
            let worker = std::thread::spawn(move || {
                let prepared = prepare(snapshot, kind, params(), token).unwrap();
                ready.send(prepared).unwrap();
                drop(returned.recv().unwrap());
                ack.send(()).unwrap();
            });
            let mut prepared = receiver.recv().unwrap();
            let held = host.admitted_bytes();
            assert!(held > 0 && Arc::strong_count(&doc) > baseline_refs);
            match outcome {
                "stale" => session.revision += 1,
                "host" => session.set_render_job_host(Some(CpuHost::cooperative())).unwrap(),
                "cancel" => cancel.cancel(),
                "queue" => {
                    // Fill the actual shared host after preparation, without
                    // minting any job in this Session's real registry.
                    let empty = || solidcraft_render::ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY };
                    for serial in 1..=2 {
                        host.submit(
                            JobId { session: 991, serial, generation: serial },
                            0,
                            empty(),
                            solidcraft_render::job::ImageRequest {
                                width: 1,
                                height: 1,
                                integrator: solidcraft_render::job::Integrator::Raster,
                                camera: Default::default(),
                                raytrace: None,
                                studio_weights: None,
                                environment: None,
                                textures: None,
                                background: [255; 3],
                                transparent_background: false,
                            },
                        )
                        .unwrap();
                    }
                }
                _ => {}
            }
            let before = host.admitted_bytes();
            let result = finish_retained(&mut session, &mut prepared);
            assert_eq!(result.is_ok(), matches!(outcome, "check" | "start"), "{outcome}");
            assert_eq!(host.admitted_bytes(), before, "completion never falsely releases prepared credit: {outcome}");
            assert!(Arc::strong_count(&doc) > baseline_refs, "completion retains scratch source: {outcome}");
            assert!(retire.try_send(prepared).is_ok());
            released.recv().unwrap();
            worker.join().unwrap();
            assert_eq!(Arc::strong_count(&doc), baseline_refs, "source's final preparation reference drops before ack: {outcome}");
            if outcome == "start" {
                assert_eq!(host.admitted_bytes(), before, "job owns transferred leases after scratch retirement");
                drain(&host);
            } else {
                assert_eq!(host.admitted_bytes(), before - held, "{outcome}");
                if outcome == "queue" {
                    drain(&host);
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native_camera_queue_preparation {
    use super::*;
    use crate::render_preparation::{batch, batch_payload};
    use solidcraft_render::job::{CancelToken, Phase};
    use std::sync::mpsc::{Receiver, SyncSender};

    fn fixture(count: usize) -> (Session, CpuHost) {
        let (mut s, host) = super::session();
        for i in 0..count {
            let camera = solidcraft_render::Camera { yaw: i as f64 * 0.2, pitch: 0.3, distance: 51.0 + i as f64, ..Default::default() };
            s.execute(
                "render.settings.set",
                &json!({"name":format!("Shot{i:02}"),"set":{
                "width":3+i,"height":2,"camera":camera,"job":{"method":"raster"}}}),
            )
            .unwrap();
        }
        s.execute("render.scene", &json!({})).unwrap();
        (s, host)
    }
    struct Worker {
        retire: SyncSender<batch::Prepared>,
        ack: Receiver<()>,
        thread: std::thread::JoinHandle<()>,
    }
    impl Worker {
        fn prepare(claim: batch::Claim) -> (batch::Prepared, Self) {
            let (ready, received) = std::sync::mpsc::sync_channel(1);
            let (retire, returned) = std::sync::mpsc::sync_channel(1);
            let (ack, released) = std::sync::mpsc::sync_channel(1);
            let thread = std::thread::spawn(move || {
                batch::with_scope(claim, |claim| {
                    ready.send(batch::prepare(claim)).unwrap();
                    drop(returned.recv().unwrap());
                });
                ack.send(()).unwrap();
            });
            (received.recv().unwrap(), Self { retire, ack: released, thread })
        }
        fn retire(self, prepared: batch::Prepared) {
            self.retire.send(prepared).unwrap();
            self.ack.recv().unwrap();
            self.thread.join().unwrap();
        }
    }
    fn initial(s: &mut Session, params: Value) -> (batch::Prepared, Worker) {
        let mut input = Some(params);
        let mut scope = None;
        let claim = batch::claim(s, &mut input, &mut scope, CancelToken::default()).unwrap();
        assert!(input.is_none());
        Worker::prepare(claim)
    }
    fn complete(host: &CpuHost) {
        for _ in 0..10000 {
            if !host.pump(4096).unwrap() {
                return;
            }
        }
        panic!("small native queue job did not finish");
    }
    fn retire_plan(plan: batch::Plan) {
        std::thread::spawn(move || batch::retire_plan(plan)).join().unwrap();
    }

    #[test]
    fn batch_arena_prepaid_exact_capacities_preserve_full_numbers_presence_and_moved_strings() {
        let mut object = serde_json::Map::new();
        let key = |name: &str| {
            let mut key = String::with_capacity(128);
            key.push_str(name);
            key
        };
        let mut text = String::with_capacity(256);
        text.push_str("owned text");
        object.insert(key("negative_zero"), json!(-0.0));
        object.insert(key("unsigned"), json!(u64::MAX));
        object.insert(key("signed"), json!(i64::MIN));
        object.insert(key("null"), Value::Null);
        object.insert(key("array"), Value::Array(vec![Value::String(text), Value::Bool(true), Value::Object(Default::default())]));
        let value = Value::Object(object);
        let expected = value.clone();
        let cancel = CancelToken::default();
        let counts = batch_payload::count(&value, &cancel).unwrap();
        let (node, edge) = batch_payload::test_element_bytes();
        let independent = (9 * node + 8 * edge + 5 * 128 + 256) as u64;
        assert_eq!(counts.bytes().unwrap(), independent);
        let payload = batch_payload::Payload::new(value, counts, &cancel).unwrap();
        assert_eq!(payload.test_capacities(), (9, 8, (5 * 128 + 256) as u64));
        assert_eq!(payload.heap_bytes().unwrap(), independent);
        let restored = payload.value(&cancel).unwrap();
        assert_eq!(restored, expected);
        assert_eq!(restored["negative_zero"].as_f64().unwrap().to_bits(), (-0.0_f64).to_bits());
        assert_eq!(restored["unsigned"].as_u64(), Some(u64::MAX));
        assert_eq!(restored["signed"].as_i64(), Some(i64::MIN));
        assert!(restored.get("missing").is_none() && restored.get("null").is_some());
        cancel.cancel();
        assert!(payload.value(&cancel).is_err());
    }

    #[test]
    fn native_batch_32_camera_semantics_real_ids_and_status_advancement_stay_metadata_only() {
        let (mut s, host) = fixture(32);
        let doc = s.doc.clone();
        let model = s.model.state();
        let camera = s.camera;
        let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len());
        let cameras: [solidcraft_render::Camera; 32] = s.doc.render.setups.values().map(|setup| setup.camera).collect::<Vec<_>>().try_into().unwrap();
        let (mut prepared, worker) = initial(&mut s, json!({}));
        assert!(prepared.is_first() && prepared.owner_matches(&s) && !prepared.committed());
        assert!(s.render_queue_state().job.is_none(), "no scratch or invented preparing job ID");
        assert!(s.render_queue_state().preparing);
        assert_eq!(s.render_queue_state().waiting, 31, "the claimed camera is separate from waiting cameras");
        let bytes = host.admitted_bytes();
        assert!(bytes > 0);
        assert_eq!(batch::finish_retained(&mut s, &mut prepared).unwrap(), json!({"queued":32}));
        let mut plan = batch::take_plan(&mut prepared).unwrap();
        let plan_bytes = plan.as_ref().unwrap().retained_bytes();
        assert!(plan_bytes > 0 && prepared.committed());
        worker.retire(prepared);
        assert_eq!(host.admitted_bytes(), bytes);
        let mut previous = None;
        for (index, expected_camera) in cameras.iter().enumerate() {
            let id = s.render_queue_state().job.unwrap();
            if let Some(previous) = previous {
                assert_eq!(id.serial, previous + 1);
            }
            previous = Some(id.serial);
            let handle = s.render_job_handle(id).unwrap();
            let status = handle.status().unwrap();
            assert_eq!(
                (status.request.width, status.request.height),
                (3, 2),
                "one first-saved common runtime setup, not every shot's different width"
            );
            assert_eq!(status.request.camera, *expected_camera);
            assert_eq!(s.render_queue_state().shot, Some(format!("Shot{index:02}")));
            complete(&host);
            assert_eq!(handle.status().unwrap().progress.phase, Phase::Completed);
            let held = host.admitted_bytes();
            for _ in 0..4 {
                s.advance_render_batch().unwrap();
                let status = s.render_jobs.peek_status(id).unwrap();
                assert_eq!(status.progress.phase, Phase::Completed);
                assert_eq!(status.request.camera, *expected_camera);
                assert_eq!(host.admitted_bytes(), held, "native metadata polling cannot collect or evict a completed result");
            }
            assert!(!host.pump(1).unwrap(), "native status polling may not enqueue/capture the next shot");
            // The ordinary result/status API deliberately collects terminal
            // owners and applies its eight-result retention policy. Keep this
            // separate from native scheduling's immutable status boundary.
            let status = s.execute("render.status", &json!({"job":id})).unwrap();
            assert_eq!(status["progress"]["phase"], "completed");
            let collected = host.admitted_bytes();
            for _ in 0..3 {
                assert_eq!(s.execute("render.status", &json!({"job":id})).unwrap()["progress"]["phase"], "completed");
                assert_eq!(host.admitted_bytes(), collected, "repeated ordinary status does not retire another result");
            }
            assert!(!host.pump(1).unwrap(), "ordinary status polling cannot prepare or enqueue the next native shot");
            if index == 31 {
                break;
            }
            assert_eq!(s.render_queue_state().waiting, 31 - index);
            let claim = batch::claim_next(&mut s, &mut plan, CancelToken::default()).unwrap().unwrap();
            assert!(plan.is_none());
            let (mut next, worker) = Worker::prepare(claim);
            assert!(!next.is_first() && next.owner_matches(&s));
            let reply = batch::finish_retained(&mut s, &mut next).unwrap();
            assert_eq!(reply["job"], json!(s.render_queue_state().job.unwrap()));
            plan = batch::take_plan(&mut next).unwrap();
            worker.retire(next);
            if index < 30 {
                assert_eq!(plan.as_ref().unwrap().retained_bytes(), plan_bytes);
            } else {
                assert!(plan.is_none());
            }
        }
        assert_eq!(s.render_queue_state().waiting, 0);
        assert!(s.render_queue_state().job.is_none());
        assert!(Arc::ptr_eq(&doc, &s.doc) && Arc::ptr_eq(&model, &s.model.state()));
        assert_eq!(s.camera, camera);
        assert_eq!((s.revision, s.evaluations, s.undo.len(), s.redo.len()), history);
    }

    #[test]
    fn native_batch_owner_and_cancelled_identical_front_cannot_publish_into_wrong_or_replacement_registry() {
        let (mut s, host) = fixture(2);
        let (mut other, _) = fixture(2);
        let (mut old, worker) = initial(&mut s, json!({}));
        let held = host.admitted_bytes();
        assert!(!old.owner_matches(&other));
        assert!(batch::finish_retained(&mut other, &mut old).is_err());
        assert_eq!(host.admitted_bytes(), held);
        let cancelled = s.execute("render.cancel", &json!({})).unwrap();
        assert_eq!(cancelled["cancel_requested"], true);
        assert_eq!(cancelled["preparing"], true);
        assert!(s.render_queue_state().job.is_none());
        let mut input = Some(json!({}));
        let mut scope = None;
        let replacement = batch::claim(&mut s, &mut input, &mut scope, CancelToken::default()).unwrap();
        assert!(batch::finish_retained(&mut s, &mut old).is_err());
        assert_eq!(s.render_queue_state().waiting, 1, "late identical front cannot cancel a replacement queue");
        assert!(s.render_queue_state().preparing);
        worker.retire(old);
        assert_eq!(host.admitted_bytes(), 0);
        let (mut current, worker) = Worker::prepare(replacement);
        batch::finish_retained(&mut s, &mut current).unwrap();
        let id = s.render_queue_state().job.unwrap();
        assert_eq!(id.serial, 1);
        assert!(other.render_job_handle(id).is_err());
        let plan = batch::take_plan(&mut current).unwrap().unwrap();
        worker.retire(current);
        batch::cancel(&mut s).unwrap();
        retire_plan(plan);
        complete(&host);
    }

    #[test]
    fn native_batch_failed_next_shot_retains_plan_credit_until_worker_ack_and_keeps_previous_result() {
        let (mut s, host) = fixture(2);
        let (mut first, worker) = initial(&mut s, json!({}));
        batch::finish_retained(&mut s, &mut first).unwrap();
        let mut plan = batch::take_plan(&mut first).unwrap();
        worker.retire(first);
        let id = s.render_queue_state().job.unwrap();
        complete(&host);
        let original = s.render_job_handle(id).unwrap().frame().unwrap().unwrap().png().to_vec();
        let plan_bytes = plan.as_ref().unwrap().retained_bytes();
        s.set_command_check(Some(Arc::new(|command, params, _| {
            if command == "render.start" && params["settings"]["camera"]["yaw"].as_f64() == Some(0.2) {
                Err("second camera policy refusal".into())
            } else {
                Ok(())
            }
        })));
        let claim = batch::claim_next(&mut s, &mut plan, CancelToken::default()).unwrap().unwrap();
        let (mut failed, worker) = Worker::prepare(claim);
        let held = host.admitted_bytes();
        assert!(batch::finish_retained(&mut s, &mut failed).unwrap_err().to_string().contains("second camera policy refusal"));
        assert_eq!(s.render_queue_state().waiting, 0);
        assert!(s.render_queue_state().job.is_none());
        assert_eq!(host.admitted_bytes(), held, "failure is not plan/source release");
        worker.retire(failed);
        assert_eq!(host.admitted_bytes(), held - plan_bytes);
        assert_eq!(s.render_job_handle(id).unwrap().frame().unwrap().unwrap().png(), original);
    }

    #[test]
    fn native_batch_individual_replace_invalidates_claim_without_consuming_scratch_job_ids() {
        let (mut s, host) = fixture(2);
        let (mut old, worker) = initial(&mut s, json!({}));
        let reply = s.execute("render.start", &json!({"settings":{"width":2,"height":2},"replace":true})).unwrap();
        let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
        assert_eq!(id.serial, 1);
        let held = host.admitted_bytes();
        assert!(batch::finish_retained(&mut s, &mut old).is_err());
        assert_eq!(host.admitted_bytes(), held);
        assert!(!old.committed());
        worker.retire(old);
        complete(&host);
        assert_eq!(s.render_job_handle(id).unwrap().status().unwrap().progress.phase, Phase::Completed);
    }
    #[test]
    fn native_batch_plan_and_first_job_obey_exact_same_pool_boundary_before_arena_publication() {
        use solidcraft_render::job::{Admission, ErrorCode, ImageExecutor, ImageHost, ImageRequest, ImageWork, JobError, JobOptions};
        struct BudgetExecutor(u64);
        impl ImageExecutor for BudgetExecutor {
            fn descriptor(&self) -> solidcraft_render::job::Result<solidcraft_render::ExecutorDescriptor> {
                let mut d = CpuHost::cooperative().descriptor()?;
                d.id = solidcraft_render::ExecutorId::new("batch-budget").map_err(|e| JobError::new(ErrorCode::Internal, e.to_string()))?;
                Ok(d)
            }
            fn memory_limit_bytes(&self) -> u64 {
                self.0
            }
            fn admit(
                &self,
                scene: &solidcraft_render::ProductScene,
                request: &ImageRequest,
                _: JobOptions,
            ) -> solidcraft_render::job::Result<Admission> {
                solidcraft_render::job::admit(scene, request)
            }
            fn begin(
                &self,
                _: solidcraft_render::ProductScene,
                _: ImageRequest,
                _: JobOptions,
                _: CancelToken,
            ) -> solidcraft_render::job::Result<Box<dyn ImageWork>> {
                Err(JobError::new(ErrorCode::Internal, "boundary test never enqueues a renderer"))
            }
        }
        let run = |limit: u64| {
            let (mut s, _) = fixture(2);
            let host = ImageHost::with_executor(Arc::new(BudgetExecutor(limit))).unwrap();
            s.set_render_job_host(Some(host.clone())).unwrap();
            let (prepared, worker) = initial(&mut s, json!({}));
            assert!(s.render_queue_state().job.is_none());
            let actual = host.admitted_bytes();
            batch::cancel(&mut s).unwrap();
            worker.retire(prepared);
            assert_eq!(host.admitted_bytes(), 0);
            actual
        };
        let exact = run(512 * 1024 * 1024);
        assert_eq!(run(exact), exact, "the exact plan+first-job pool boundary admits without hidden backing");
        assert!(
            run(exact - 1) < exact - 1,
            "one-byte shortage refuses the new arena lease and leaves only the retained failed preparation until ack"
        );
    }

    #[test]
    fn native_batch_idle_cancel_and_host_replacement_keep_plan_until_explicit_worker_retirement() {
        for replace_host in [false, true] {
            let (mut s, host) = fixture(2);
            let (mut first, worker) = initial(&mut s, json!({}));
            batch::finish_retained(&mut s, &mut first).unwrap();
            let mut plan = batch::take_plan(&mut first).unwrap();
            worker.retire(first);
            complete(&host);
            let bytes = plan.as_ref().unwrap().retained_bytes();
            let held = host.admitted_bytes();
            if replace_host {
                s.set_render_job_host(Some(CpuHost::cooperative())).unwrap();
            } else {
                batch::cancel(&mut s).unwrap();
            }
            assert!(batch::claim_next(&mut s, &mut plan, CancelToken::default()).is_err());
            assert!(plan.is_some(), "wrong owner/stale queue refuses without dropping heavy plan on UI");
            let before = host.admitted_bytes();
            assert!(before >= bytes);
            retire_plan(plan.take().unwrap());
            assert_eq!(host.admitted_bytes(), before - bytes);
            assert!(held >= bytes);
        }
    }
    #[test]
    fn stale_idle_plan_cancellation_cannot_stop_an_identical_new_queue() {
        let (mut s, host) = fixture(2);
        let (mut first, worker) = initial(&mut s, json!({}));
        batch::finish_retained(&mut s, &mut first).unwrap();
        let stale = batch::take_plan(&mut first).unwrap().unwrap();
        worker.retire(first);
        batch::cancel(&mut s).unwrap();
        complete(&host);
        let mut params = Some(json!({}));
        let mut scope = None;
        let next = batch::claim(&mut s, &mut params, &mut scope, CancelToken::default()).unwrap();
        assert!(stale.owner_matches(&s), "same real owner does not prove the current queue epoch");
        assert!(!batch::cancel_plan(&mut s, &stale).unwrap());
        assert_eq!(s.render_queue_state().waiting, 1);
        assert!(s.render_queue_state().preparing);
        retire_plan(stale);
        let (mut current, worker) = Worker::prepare(next);
        batch::finish_retained(&mut s, &mut current).unwrap();
        let plan = batch::take_plan(&mut current).unwrap().unwrap();
        worker.retire(current);
        assert!(batch::cancel_plan(&mut s, &plan).unwrap());
        retire_plan(plan);
        complete(&host);
    }
    #[test]
    fn native_batch_replace_design_keeps_only_hosts_and_retires_old_plan_under_its_original_lease() {
        let (mut s, host) = fixture(2);
        let (mut first, worker) = initial(&mut s, json!({}));
        batch::finish_retained(&mut s, &mut first).unwrap();
        let mut plan = batch::take_plan(&mut first).unwrap();
        worker.retire(first);
        let bytes = plan.as_ref().unwrap().retained_bytes();
        s.replace_design(Session::default());
        assert_eq!(s.render_queue_state().waiting, 0);
        assert!(s.render_queue_state().job.is_none());
        assert!(!plan.as_ref().unwrap().owner_matches(&s));
        assert!(batch::claim_next(&mut s, &mut plan, CancelToken::default()).is_err());
        assert!(plan.is_some());
        let before = host.admitted_bytes();
        retire_plan(plan.take().unwrap());
        assert_eq!(host.admitted_bytes(), before - bytes);
        complete(&host);
    }
    #[test]
    fn native_batch_rechecks_arbitrary_current_guard_with_full_arena_restored_negative_zero_params() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (mut s, host) = fixture(2);
        let allow = Arc::new(AtomicBool::new(true));
        let policy = allow.clone();
        let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = calls.clone();
        let uid = s.uid;
        s.set_command_check(Some(Arc::new(move |id, params, owner| {
            if id == "render.batch" || id == "render.start" {
                let request = if id == "render.batch" { &params["request"] } else { params };
                assert_eq!(request["studio_weights"][0].as_f64().unwrap().to_bits(), (-0.0_f64).to_bits());
                assert_eq!(owner.uid, uid);
                observed.lock().unwrap().push((id.to_string(), std::thread::current().id()));
                if id == "render.start" && !policy.load(Ordering::Acquire) {
                    return Err("current mutable callback refused the next camera".into());
                }
            }
            Ok(())
        })));
        let params = json!({"request":{"settings":{"width":3,"height":2},"studio_weights":[-0.0,0.5,1.0]}});
        let (mut first, worker) = initial(&mut s, params);
        batch::finish_retained(&mut s, &mut first).unwrap();
        let mut plan = batch::take_plan(&mut first).unwrap();
        worker.retire(first);
        complete(&host);
        let claim = batch::claim_next(&mut s, &mut plan, CancelToken::default()).unwrap().unwrap();
        let (mut next, worker) = Worker::prepare(claim);
        let held = host.admitted_bytes();
        allow.store(false, Ordering::Release);
        assert!(batch::finish_retained(&mut s, &mut next).unwrap_err().to_string().contains("mutable callback"));
        assert!(!next.committed());
        assert_eq!(host.admitted_bytes(), held);
        worker.retire(next);
        assert_eq!(s.render_queue_state().waiting, 0);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 6);
        let window = std::thread::current().id();
        assert_eq!(calls[0].0, "render.batch");
        assert_eq!(calls[0].1, window);
        assert_ne!(calls[1].1, window);
        assert_eq!(calls[2].0, "render.batch");
        assert_eq!(calls[2].1, window);
        assert_eq!(calls[3].0, "render.start");
        assert_eq!(calls[3].1, window);
        assert_ne!(calls[4].1, window);
        assert_eq!(calls[5].1, window);
    }

    #[test]
    fn native_batch_first_commit_rechecks_only_batch_refusal_with_exact_moved_input_and_retains_credit() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let (mut s, host) = fixture(2);
        let mut method = String::with_capacity(512);
        method.push_str("raster");
        let params = json!({"request":{"settings":{"width":3,"height":2,"job":{"method":method}},"studio_weights":[-0.0,0.5,1.0]}});
        let original_method = params["request"]["settings"]["job"]["method"].as_str().unwrap().as_ptr() as usize;
        let expected = params.clone();
        let allow = Arc::new(AtomicBool::new(true));
        let policy = allow.clone();
        let batches = Arc::new(AtomicUsize::new(0));
        let batch_calls = batches.clone();
        let starts = Arc::new(AtomicUsize::new(0));
        let start_calls = starts.clone();
        let uid = s.uid;
        s.set_command_check(Some(Arc::new(move |id, input, owner| {
            assert_eq!(owner.uid, uid);
            if id == "render.batch" {
                batch_calls.fetch_add(1, Ordering::AcqRel);
                assert_eq!(*input, expected);
                assert_eq!(
                    input["request"]["settings"]["job"]["method"].as_str().unwrap().as_ptr() as usize,
                    original_method,
                    "the original raw input moves; guard metadata is not reconstructed or cloned"
                );
                assert_eq!(input["request"]["studio_weights"][0].as_f64().unwrap().to_bits(), (-0.0_f64).to_bits());
                if !policy.load(Ordering::Acquire) {
                    return Err("current Batch-only policy refuses this first camera".into());
                }
            } else if id == "render.start" {
                start_calls.fetch_add(1, Ordering::AcqRel);
            }
            Ok(())
        })));
        let (mut first, worker) = initial(&mut s, params);
        assert_eq!(batches.load(Ordering::Acquire), 1);
        assert_eq!(starts.load(Ordering::Acquire), 1, "worker Start policy remains allowed");
        let held = host.admitted_bytes();
        assert!(held > 0);
        allow.store(false, Ordering::Release);
        let error = batch::finish_retained(&mut s, &mut first).unwrap_err();
        assert!(error.to_string().contains("Batch-only policy"));
        assert_eq!(batches.load(Ordering::Acquire), 2, "current real-owner Batch guard runs at first commit");
        assert_eq!(starts.load(Ordering::Acquire), 1, "Batch refusal occurs before any actual Start enqueue");
        assert!(!first.committed());
        assert!(s.render_retained_results().unwrap().is_empty());
        assert!(s.render_queue_state().job.is_none());
        assert_eq!((s.render_queue_state().running, s.render_queue_state().waiting), (0, 0));
        assert_eq!(host.admitted_bytes(), held, "window-side refusal retains all source, request and plan leases");
        worker.retire(first);
        assert_eq!(host.admitted_bytes(), 0, "same worker drops ownership before acknowledgement");
        s.set_command_check(None);
        let reply = s.execute("render.start", &json!({"settings":{"width":1,"height":1,"job":{"method":"raster"}}})).unwrap();
        let job: solidcraft_render::job::JobId = serde_json::from_value(reply["job"].clone()).unwrap();
        assert_eq!(job.serial, 1, "refused Batch and scratch preparation consumed no actual registry identity");
        complete(&host);
    }

    #[test]
    fn native_batch_worker_context_survives_plan_take_until_source_drop_and_release_ack() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct DropInScope {
            path: String,
            seen: Arc<AtomicBool>,
        }
        impl Drop for DropInScope {
            fn drop(&mut self) {
                let scope = solidcraft_io::vfs::current_scope().expect("source destruction stays in its original worker permission context");
                scope.check(&self.path).unwrap();
                self.seen.store(true, Ordering::Release);
            }
        }
        let (mut s, host) = fixture(2);
        let seen = Arc::new(AtomicBool::new(false));
        let probe =
            DropInScope { path: std::env::temp_dir().join("solidcraft-batch-scope-drop.hdr").to_string_lossy().into_owned(), seen: seen.clone() };
        s.set_command_check(Some(Arc::new(move |_, _, _| {
            assert!(!probe.path.is_empty());
            Ok(())
        })));
        let mut params = Some(json!({}));
        let mut scope = Some(solidcraft_io::vfs::PathScope::new([std::env::temp_dir()]));
        let claim = batch::claim(&mut s, &mut params, &mut scope, CancelToken::default()).unwrap();
        let (mut prepared, worker) = Worker::prepare(claim);
        batch::finish_retained(&mut s, &mut prepared).unwrap();
        let plan = batch::take_plan(&mut prepared).unwrap().unwrap();
        s.set_command_check(None);
        assert!(!seen.load(Ordering::Acquire));
        worker.retire(prepared);
        assert!(seen.load(Ordering::Acquire), "ack follows source destruction inside original scope even though Plan moved to UI");
        batch::cancel_plan(&mut s, &plan).unwrap();
        retire_plan(plan);
        complete(&host);
    }

    #[test]
    fn pending_batch_cancel_policy_check_uses_the_same_typed_params_and_current_host_refusal() {
        let (mut s, host) = fixture(1);
        let doc = s.doc.clone();
        let before = (s.revision, s.undo.len(), s.redo.len());
        assert_eq!(batch::check_cancel(&s, &json!({})).unwrap(), None);
        assert!(batch::check_cancel(&s, &json!({"unexpected":true})).is_err());
        assert!(batch::check_cancel(&s, &json!({"job":{"session":999,"serial":1,"generation":1}})).is_err());
        s.set_command_check(Some(Arc::new(|id, _, _| if id == "render.cancel" { Err("embedding cancellation refusal".into()) } else { Ok(()) })));
        assert!(batch::check_cancel(&s, &json!({})).unwrap_err().to_string().contains("embedding cancellation refusal"));
        assert!(Arc::ptr_eq(&doc, &s.doc));
        assert_eq!((s.revision, s.undo.len(), s.redo.len()), before);
        assert_eq!(s.render_queue_state().waiting, 0);
        assert_eq!(host.admitted_bytes(), 0);
    }
}

#[cfg(all(feature = "raytrace-cpu", not(target_arch = "wasm32")))]
#[test]
fn tessellated_cpu_sheet_saved_and_prospective_bindings_certify_and_render_without_saving_the_draft() {
    use solidcraft_doc::render_optics::{OpticalBoundary, OpticalMaterial, SavedOptics};
    use solidcraft_render::job::{ImageRequest, Phase};
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let directory = std::env::temp_dir().join(format!("solidcraft-cpu-path-tiled-sheets-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let mut s = Session::default();
    for (name, offset) in [("SavedSheet", 0), ("DraftSheet", 30)] {
        let path = directory.join(format!("{name}.obj"));
        let mut source = format!("o {name}\n");
        for y in 0..2 {
            for x in 0..2 {
                let x = offset - 10 + x * 10;
                let y = -10 + y * 10;
                source.push_str(&format!("v {x} {y} 0\nv {} {y} 0\nv {} {} 0\nv {x} {} 0\n", x + 10, x + 10, y + 10, y + 10));
            }
        }
        for cell in 0..4 {
            let index = cell * 4 + 1;
            source.push_str(&format!("f {index} {} {}\nf {index} {} {}\n", index + 1, index + 2, index + 2, index + 3));
        }
        std::fs::write(&path, source).unwrap();
        s.execute("file.insert_mesh", &json!({"path":path})).unwrap();
    }
    let scene = crate::product_scene::extract(&s).unwrap();
    assert_eq!(scene.meshes.len(), 2);
    assert!(scene.meshes.iter().all(|body| body.mesh.triangles.len() == 8));
    let saved_name = scene.meshes[0].name.clone();
    let draft_name = scene.meshes[1].name.clone();
    let targets = s.execute("render.library.targets", &json!({})).unwrap();
    let draft_target = targets["items"].as_array().unwrap().iter().find(|row| row["name"] == draft_name).unwrap()["stable_target"].clone();
    let mut camera = solidcraft_render::Camera { target: solidcore_record::Vec3::new(15.0, 0.0, 0.0), distance: 30.0, ..Default::default() };
    camera.set_view(solidcraft_render::StandardView::Top);
    s.execute(
        "render.settings.set",
        &json!({"name":"Sheets","set":{"camera":camera,"width":8,"height":4,
        "job":{"method":"cpu_raytrace","options":{"path":{"max_depth":8},"progressive":{"samples_per_pixel":4,"seed":81},
        "lights":[],"environment":{"color":[1,1,1],"intensity":1},"output":{"linear_exr":true}}}}}),
    )
    .unwrap();
    s.execute(
        "render.optics.bind",
        &json!({"name":"Sheets","body_index":0,"body_name":saved_name,"expected_revision":s.revision,
        "optics":{"boundary":"thin_sheet","ior":1,"thickness_mm":2,"absorption_per_mm":[0.1,0.2,0]}}),
    )
    .unwrap();
    let persisted = solidcraft_io::write_design_container(&s.doc).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    s = Session::new(solidcraft_io::read_design(&persisted).unwrap());
    s.execute("render.scene", &json!({})).unwrap();
    let saved = s.doc.render.setups["Sheets"].clone();
    let mut draft = saved.clone();
    draft
        .optics
        .set(
            SavedOptics {
                target: serde_json::from_value(draft_target).unwrap(),
                optics: OpticalMaterial {
                    boundary: OpticalBoundary::ThinSheet,
                    ior: 1.0,
                    thickness_mm: 2.0,
                    absorption_per_mm: [0.3, 0.0, 0.2],
                    ..Default::default()
                },
            },
            false,
        )
        .unwrap();
    let host = CpuHost::for_platform().unwrap();
    s.set_render_job_host(Some(host.clone())).unwrap();
    let document = s.doc.clone();
    let geometry = s.model.state();
    let history = (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty());
    let named = s.execute("render.preflight", &json!({"setup":"Sheets"})).unwrap();
    let checked = s.execute("render.preflight", &json!({"settings":draft})).unwrap();
    assert_eq!(named["request"]["raytrace"]["dielectrics"].as_array().unwrap().len(), 1);
    let captured: ImageRequest = serde_json::from_value(checked["request"].clone()).unwrap();
    let optics = &captured.raytrace.as_ref().unwrap().dielectrics;
    assert_eq!(optics.len(), 2, "prospective stable optics add to the saved binding");
    for (name, absorption) in [(&saved_name, [0.1, 0.2, 0.0]), (&draft_name, [0.3, 0.0, 0.2])] {
        let input = optics.iter().find(|input| input.body_name == *name).unwrap();
        assert_eq!(input.boundary, solidcraft_render::job::DielectricBoundary::ThinSheet);
        assert_eq!((input.ior, input.thickness_mm, input.absorption_per_mm), (1.0, 2.0, absorption));
    }
    let runtime = json!({"integrator":"raytrace","settings":{"camera":camera,"width":8,"height":4},"raytrace":captured.raytrace});
    assert_eq!(s.execute("render.preflight", &runtime).unwrap()["request"], checked["request"]);
    let mut legacy = json!({"settings":draft});
    legacy["settings"]["job"]["options"].as_object_mut().unwrap().remove("path");
    assert!(
        s.execute("render.preflight", &legacy).unwrap_err().to_string().contains("unsupported dielectric boundary tessellation"),
        "successful Path checks must actually certify the eight-triangle mesh, not just forward optics metadata"
    );
    assert_eq!(host.admitted_bytes(), 0, "Check releases its complete preparation certificate");
    let run = |s: &mut Session, params: Value| {
        let started = s.execute("render.start", &params).unwrap();
        let job = serde_json::from_value(started["job"].clone()).unwrap();
        let handle = s.render_job_handle(job).unwrap();
        let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{:?}", status.error);
        let frame = handle.frame().unwrap().unwrap();
        (frame.png().to_vec(), frame.linear_exr().unwrap().to_vec())
    };
    let original = run(&mut s, json!({"setup":"Sheets"}));
    let prospective = run(&mut s, json!({"settings":draft,"replace":true}));
    assert_ne!(original, prospective, "the second prospective optical assignment must change the actual pixels");
    let mut runtime = runtime;
    runtime["replace"] = json!(true);
    assert_eq!(prospective, run(&mut s, runtime));
    assert_eq!(s.doc.render.setups["Sheets"], saved);
    assert!(Arc::ptr_eq(&document, &s.doc) && Arc::ptr_eq(&geometry, &s.model.state()));
    assert_eq!(history, (s.revision, s.evaluations, s.undo.len(), s.redo.len(), s.is_dirty()));
}
