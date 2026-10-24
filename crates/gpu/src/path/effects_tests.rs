//! Device execution for layered weathering and nondestructive presentation.
use super::*;
use serde_json::json;
use solidcraft_render::job::{
    JobId, Phase,
    aging::{AgingKind, AgingLayer, AgingStack, BodyName, BodyTarget},
    finishing::Finishing,
    looks::{LookKind, LookLayer, LookStack},
    photographic::{Flare, Photographic},
};

#[test]
fn saved_targeted_aging_reaches_preflight_and_start_after_cold_reopen() {
    let Some(executor) = executor() else { return };
    assert!(executor.supports_aging());
    let mut session = solidcraft_engine::Session::default();
    session.execute("solid.box", &json!({"body_name":"Decoy","length":1,"width":1,"height":1})).unwrap();
    session.execute("solid.box", &json!({"body_name":"Target panel","length":20,"width":15,"height":6})).unwrap();
    let state = session.model.state();
    let feature = state.bodies.iter().find(|body| body.name == "Target panel").unwrap().feature;
    let target = json!({"definition":"Target panel","feature":feature,"component":0,"occurrence_path":[]});
    session
        .execute(
            "render.settings.set",
            &json!({"name":"Aging","set":{
                "width":32,"height":24,"job":{"method":"gpu_path","options":{
                    "samples":4,"seed":714,"max_depth":4,"linear_exr":true,
                    "lighting":{"environment":[8,8,8]},
                    "aging":[{"kind":"rust","strength":1,"coverage":1,"target":target}]
                }}
            }}),
        )
        .unwrap();
    let native = solidcraft_io::write_design_container(&session.doc).unwrap();
    session = solidcraft_engine::Session::new(solidcraft_io::read_design(&native).unwrap());
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    session.register_render_job_host(host.clone()).unwrap();
    let document = session.doc.clone();
    let revision = session.revision;
    let original = session.execute("render.preflight", &json!({"setup":"Aging"})).unwrap();
    assert_eq!(host.admitted_bytes(), 0, "Check must not reserve a job");

    // Removing another captured body changes the ordinal, never the saved target.
    session.visibility.hidden_bodies.push("Decoy".into());
    let physical = session.execute("component.properties", &json!({})).unwrap();
    let checked = session.execute("render.preflight", &json!({"setup":"Aging"})).unwrap();
    let runtime_target = &checked["options"]["aging"][0]["target"];
    assert_eq!(runtime_target["body_name"], "Target panel");
    assert_ne!(runtime_target["body_index"], original["options"]["aging"][0]["target"]["body_index"]);
    let saved = session.doc.render.setups["Aging"].clone();
    assert_eq!(checked, session.execute("render.preflight", &json!({"settings":saved})).unwrap());
    assert_eq!(host.admitted_bytes(), 0);

    let started = session.execute("render.start", &json!({"setup":"Aging"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = session.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{status:?}");
    assert_eq!(json!(status.options.aging), checked["options"]["aging"]);
    let aged = handle.frame().unwrap().unwrap();

    // Execute the same shot with the layer disabled: successful admission alone
    // cannot prove that the producer actually evaluates aging in transport.
    let mut disabled = serde_json::to_value(&session.doc.render.setups["Aging"]).unwrap();
    disabled["job"]["options"]["aging"][0]["enabled"] = json!(false);
    let started = session.execute("render.start", &json!({"settings":disabled})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = session.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{status:?}");
    let unaged = handle.frame().unwrap().unwrap();
    assert_ne!(aged.png(), unaged.png());
    assert_ne!(aged.linear_exr().unwrap(), unaged.linear_exr().unwrap(), "aging must change transport, not only presentation");
    assert!(Arc::ptr_eq(&document, &session.doc));
    assert_eq!(revision, session.revision);
    assert_eq!(physical, session.execute("component.properties", &json!({})).unwrap());

    session.visibility.hidden_bodies.push("Target panel".into());
    for command in ["render.preflight", "render.start"] {
        assert!(session.execute(command, &json!({"setup":"Aging"})).unwrap_err().to_string().contains("hidden"));
    }
}

#[test]
fn opaque_aging_changes_transport_but_disabled_layers_and_passive_stage_are_exact_identities() {
    let Some(executor) = executor() else { return };
    let options = JobOptions { samples: 16, seed: 42, max_depth: 4, linear_exr: true, ..Default::default() };
    let baseline = complete_linear_fixture(executor.context.clone(), options);
    let mut aging = AgingStack::default();
    aging.push(AgingLayer { enabled: false, ..Default::default() }).unwrap();
    assert_eq!(complete_linear_fixture(executor.context.clone(), JobOptions { aging, ..options }), baseline);
    *aging.get_mut(0).unwrap() = AgingLayer { strength: 1.0, coverage: 0.65, scale_mm: 0.7, ..Default::default() };
    let aged = complete_linear_fixture(executor.context.clone(), JobOptions { aging, ..options });
    assert_ne!(aged.0, baseline.0);
    assert_ne!(aged.1, baseline.1, "surface aging participates in transport rather than grading PNG");
    let denoised = complete_linear_fixture(executor.context.clone(), JobOptions { aging, denoise: true, ..options });
    assert_eq!(aged.1, denoised.1, "guides may not mutate the raw aging master");
    let render = |mut source: ProductScene, aging: AgingStack| {
        source.meshes[0].name = "__render_stage__/test".into();
        let mut task = Task::new(executor.context.clone(), source, request(16), JobOptions { aging, ..options }, CancelToken::default()).unwrap();
        for _ in 0..4096 {
            if let TaskPoll::Complete(png) = task.step(4096).unwrap() {
                return (png, task.take_linear_exr());
            }
        }
        panic!("bounded stage fixture did not complete")
    };
    assert_eq!(render(sphere(white()), aging), render(sphere(white()), Default::default()), "passive stages are not aged implicitly");
}

#[test]
fn weathering_layer_order_and_upward_deposition_are_executed_in_the_shader() {
    let Some(executor) = executor() else { return };
    let context = executor.context;
    let shader = format!(
        "{SHADER}\n{}",
        r#"
@compute @workgroup_size(64)
fn weathering_reference(@builtin(global_invocation_id) gid:vec3<u32>){
    if gid.x>=4u{return;}
    var t=triangle(0u);
    var normal=vec3<f32>(0.0,0.0,1.0);
    if gid.x==1u{normal=-normal;}
    if gid.x==2u{t.base.w=1.0;}
    if gid.x==3u{t.interior.z=1.0;}
    let result=aged_surface(t,vec3<f32>(0.125,0.25,0.5),normal);
    accumulation[gid.x]=vec4<f32>(result.base.xyz,1.0);
}
"#
    );
    let module = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(shader.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &module,
        entry_point: Some("weathering_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut dust = AgingLayer::preset(AgingKind::Dust);
    dust.strength = 1.0;
    dust.coverage = 1.0;
    dust.upward_bias = 1.0;
    let mut stack = AgingStack::default();
    stack.push(AgingLayer { strength: 1.0, coverage: 1.0, ..Default::default() }).unwrap();
    stack.push(dust).unwrap();
    let mut seen = Vec::new();
    for _ in 0..2 {
        let mut task =
            Task::new(context.clone(), sphere(white()), request(2), JobOptions { aging: stack, ..Default::default() }, CancelToken::default())
                .unwrap();
        context.queue.write_buffer(&task.uniform, 0, &task.parameters(4));
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let pixels = task.read_linear().unwrap();
        assert_eq!(pixels[2], [1.0; 4], "glass is untouched");
        assert_eq!(pixels[3], [1.0; 4], "render stage is untouched");
        seen.push(pixels);
        stack.move_layer(0, 1).unwrap();
    }
    let expected = dust.color_srgb.map(solidcraft_render::surface::srgb8_to_linear);
    for (actual, expected) in seen[0][0][..3].iter().zip(expected) {
        assert!((f64::from(*actual) - expected).abs() < 1e-6);
    }
    assert_ne!(seen[0][0], seen[0][1], "upward dust differs from downward rust");
    assert_ne!(seen[0][0], seen[1][0], "layer order changes the final material");
}

#[test]
fn aging_rejects_stale_or_glass_targets_and_photographic_alpha_is_never_flattened() {
    let source = sphere(white());
    let mut aging = AgingStack::default();
    aging
        .push(AgingLayer { target: Some(BodyTarget { body_index: 0, body_name: BodyName::new("wrong sphere").unwrap() }), ..Default::default() })
        .unwrap();
    assert!(admission(&source, &request(16), JobOptions { aging, ..Default::default() }).unwrap_err().message.contains("identity"));
    aging.get_mut(0).unwrap().target = Some(BodyTarget { body_index: 0, body_name: BodyName::new("original sphere").unwrap() });
    assert!(admission(&source, &request(16), JobOptions { aging, ..Default::default() }).is_ok());
    let mut glass = source.clone();
    glass.meshes[0].transmission = Some(clear_glass());
    assert!(admission(&glass, &request(16), JobOptions { aging, ..Default::default() }).is_err());
    let mut req = request(16);
    req.transparent_background = true;
    let options = JobOptions {
        finishing: Finishing { photographic: Photographic { grain: 0.01, ..Default::default() }, ..Default::default() },
        ..Default::default()
    };
    assert!(admission(&source, &req, options).unwrap_err().message.contains("opaque"));
}

#[test]
fn photographic_and_ordered_colour_effects_change_png_not_raw_master_and_regrade_matches() {
    let Some(executor) = executor() else { return };
    let mut looks = LookStack::default();
    looks.push(LookLayer { kind: LookKind::Warm, strength: 0.7, ..Default::default() }).unwrap();
    looks.push(LookLayer { kind: LookKind::Faded, strength: 0.25, ..Default::default() }).unwrap();
    let finish = Finishing {
        looks,
        photographic: Photographic {
            flare: Flare { strength: 0.8, threshold: 0.2, ..Default::default() },
            chromatic_pixels: 1.2,
            radial_distortion: 0.03,
            soft_focus: 0.1,
            sharpening: 0.2,
            grain: 0.025,
            seed: 77,
        },
        ..Default::default()
    };
    let options = JobOptions { samples: 16, seed: 713, linear_exr: true, ..Default::default() };
    let base = complete_linear_fixture(executor.context.clone(), options);
    let photo = complete_linear_fixture(executor.context.clone(), JobOptions { finishing: finish, ..options });
    assert_ne!(base.0, photo.0);
    assert_eq!(base.1, photo.1);
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let mut session = solidcraft_engine::Session::default();
    session.register_render_job_host(host).unwrap();
    session.execute("solid.box", &json!({"length":20,"width":15,"height":6})).unwrap();
    let request =
        json!({"progressive":true,"samples":16,"seed":714,"linear_exr":true,"settings":{"width":32,"height":24},"lighting":{"environment":[8,8,8]}});
    let first = session.execute("render.start", &request).unwrap();
    let id: JobId = serde_json::from_value(first["job"].clone()).unwrap();
    let original_handle = session.render_job_handle(id).unwrap();
    assert_eq!(original_handle.wait_timeout(Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
    let original = original_handle.frame().unwrap().unwrap();
    let revision = session.revision;
    let document = session.doc.to_json();
    let graded = session.execute("render.result.regrade", &json!({"job":id,"presentation":{"finishing":finish,"background":[240,243,248]}})).unwrap();
    let id: JobId = serde_json::from_value(graded["job"].clone()).unwrap();
    let handle = session.render_job_handle(id).unwrap();
    assert_eq!(handle.wait_timeout(Duration::from_secs(60)).unwrap().progress.phase, Phase::Completed);
    let frame = handle.frame().unwrap().unwrap();
    assert_eq!(frame.linear_exr().unwrap().as_ptr(), original.linear_exr().unwrap().as_ptr());
    assert_ne!(frame.png(), original.png());
    assert_eq!(session.revision, revision);
    assert_eq!(session.doc.to_json(), document);
    assert_eq!(frame.presentation().unwrap().presentation.finishing, finish);
}

/// Saved identities are resolved by the engine and consumed by the actual GPU,
/// not by a raster-backed test double claiming an unsupported capability.
#[test]
fn saved_aging_identity_reaches_real_gpu_after_cold_reopen() {
    use solidcraft_engine::{Session, doc, io};
    let Some(executor) = executor() else { return };
    assert!(executor.supports_aging());
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let mut source = Session::default();
    source.execute("solid.box", &json!({"body_name":"Before","length":1,"width":1,"height":1})).unwrap();
    let created = source.execute("solid.box", &json!({"body_name":"Target panel","length":2,"width":2,"height":1})).unwrap();
    let target = doc::render_surface::RenderBodyTarget {
        definition: "Target panel".into(),
        feature: serde_json::from_value(created["feature"].clone()).unwrap(),
        component: 0,
        occurrence_path: Vec::new(),
    };
    let mut layers = doc::render_aging::SavedAgingStack::default();
    layers.push(AgingLayer { strength: 1.0, coverage: 1.0, ..Default::default() }).unwrap();
    layers.get_mut(0).unwrap().target = Some(target.clone());
    source
        .execute(
            "render.settings.set",
            &json!({"name":"Aging","set":{
                "width":16,"height":16,"camera":request(16).camera,
                "job":{"method":"gpu_path","options":{"samples":8,"max_depth":4,"seed":42,"aging":layers}}
            }}),
        )
        .unwrap();
    let json = source.doc.to_json();
    assert!(!json.contains("body_index"));
    assert_eq!(doc::Document::from_json(&json).unwrap().render, source.doc.render);
    let bytes = io::write_design_container(&source.doc).unwrap();
    let reopened = io::read_design(&bytes).unwrap();
    assert_eq!(reopened.render, source.doc.render);
    let mut session = Session::new(reopened);
    session.set_render_job_host(Some(ImageHost::cooperative())).unwrap();
    session.register_render_job_host(host).unwrap();
    let first = session.execute("render.preflight", &json!({"setup":"Aging"})).unwrap();
    // Removing an unrelated earlier capture entry must not change saved identity.
    session.visibility.hidden_bodies.push("Before".into());
    let checked = session.execute("render.preflight", &json!({"setup":"Aging"})).unwrap();
    assert_ne!(first["options"]["aging"][0]["target"]["body_index"], checked["options"]["aging"][0]["target"]["body_index"]);
    assert_eq!(checked["options"]["aging"][0]["target"]["body_name"], "Target panel");
    let saved = session.doc.render.setups["Aging"].clone();
    assert_eq!(checked, session.execute("render.preflight", &json!({"settings":saved})).unwrap());
    let physical = session.execute("component.properties", &json!({})).unwrap();
    let before = session.doc.clone();
    let history = (session.revision, session.undo.len(), session.redo.len());
    let started = session.execute("render.start", &json!({"setup":"Aging"})).unwrap();
    let id: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let handle = session.render_job_handle(id).unwrap();
    let status = handle.wait_timeout(Duration::from_secs(60)).unwrap();
    assert_eq!(status.progress.phase, Phase::Completed, "{status:?}");
    assert_eq!(json!(status.options.aging), checked["options"]["aging"]);
    assert!(handle.frame().unwrap().is_some());
    assert!(Arc::ptr_eq(&before, &session.doc));
    assert_eq!((session.revision, session.undo.len(), session.redo.len()), history);
    assert_eq!(session.execute("component.properties", &json!({})).unwrap(), physical);
    session.visibility.hidden_bodies.push("Target panel".into());
    for command in ["render.preflight", "render.start"] {
        assert!(session.execute(command, &json!({"setup":"Aging"})).unwrap_err().to_string().contains("hidden"));
    }
}
