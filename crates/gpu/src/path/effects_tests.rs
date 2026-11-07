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
use wgpu::util::DeviceExt;

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
        context.queue.write_buffer(&task.uniform, 0, &task.parameters(4).unwrap());
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

#[test]
fn scratch_world_point_is_invariant_when_only_camera_target_crosses_a_noise_period() {
    let Some(executor) = executor() else { return };
    let context = executor.context;
    let mut stack = AgingStack::default();
    stack
        .push(AgingLayer {
            kind: AgingKind::Scratches,
            scale_mm: 2.0,
            strength: 1.0,
            coverage: 1.0,
            upward_bias: 0.0,
            color_srgb: [255, 0, 0],
            ..Default::default()
        })
        .unwrap();
    let mut images = Vec::new();
    for target_y in [0.0, 8192.0] {
        // Every point/translation is dyadic and exactly representable in f32.
        // Only the camera's world origin changes; no geometry, RNG or tolerance
        // changes can explain a different authored Scratch band at that point.
        let reference = format!(
            r#"
@compute @workgroup_size(64)
fn scratch_world_reference(@builtin(global_invocation_id) gid:vec3<u32>){{
    if gid.x>=16u{{return;}}
    let world=vec3<f32>(2.0*f32(gid.x%4u)/128.0,2.0*(2.875+1.25*f32(gid.x/4u)),0.0);
    let relative=world-vec3<f32>(0.0,{target_y:.1},0.0);
    let result=aged_surface(triangle(0u),relative,vec3<f32>(0.0,0.0,1.0));
    accumulation[gid.x]=vec4<f32>(result.base.xyz,1.0);
}}
"#
        );
        let shader = format!("{SHADER}\n{reference}");
        let module =
            context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(shader.into()) });
        let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&context.layout)],
            immediate_size: 0,
        });
        let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&layout),
            module: &module,
            entry_point: Some("scratch_world_reference"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mut req = request(4);
        req.camera.target = solidcore_record::Vec3::new(0.0, target_y, 0.0);
        let mut task =
            Task::new(context.clone(), sphere(white()), req, JobOptions { aging: stack, ..Default::default() }, CancelToken::default()).unwrap();
        context.queue.write_buffer(&task.uniform, 0, &task.parameters(16).unwrap());
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        images.push(task.read_linear().unwrap());
    }
    assert!(images[0].iter().any(|pixel| pixel[1] < 0.99), "the fixed world-point oracle must include visible Scratch coverage");
    assert_eq!(images[0], images[1], "moving only the camera may not select different world-anchored Scratch bands");
}

#[test]
fn scratch_integer_band_matches_independent_f64_boundaries_and_signed_large_cells() {
    let Some(executor) = executor() else { return };
    let context = executor.context;
    let mut cases = Vec::new();
    for band in -256_i32..=256 {
        let point = band as f32 * 1.25;
        let bits = point.to_bits();
        let neighbours = if point == 0.0 {
            [-1.0 / 1_048_576.0, 0.0, 1.0 / 1_048_576.0]
        } else if point > 0.0 {
            [f32::from_bits(bits - 1), point, f32::from_bits(bits + 1)]
        } else {
            [f32::from_bits(bits + 1), point, f32::from_bits(bits - 1)]
        };
        for value in neighbours {
            for carry in 0..5_u32 {
                cases.push([value.to_bits(), carry]);
            }
        }
    }
    for point in [-200000000000.0_f32, -1000000000.0, -16777218.0, -5120.25, 4095.9998, 5120.25, 16777218.0, 1000000000.0, 200000000000.0] {
        for carry in 0..5_u32 {
            cases.push([point.to_bits(), carry]);
        }
    }
    let mut random = 0xa903c717_u32;
    while cases.len() < 8192 {
        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
        let exponent = 110 + (random >> 8) % 54;
        let bits = (random & 0x807fffff) | (exponent << 23);
        cases.push([bits, random % 5]);
    }
    let mut bytes = Vec::with_capacity(4 + cases.len() * 8);
    bytes.extend((cases.len() as u32).to_le_bytes());
    for case in &cases {
        for word in case {
            bytes.extend(word.to_le_bytes());
        }
    }
    let input = context.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("independent Scratch band inputs"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE,
    });
    let source = format!(
        "{SHADER}\n{}",
        r#"
@compute @workgroup_size(32)
fn scratch_band_reference(@builtin(global_invocation_id) gid:vec3<u32>){
    if gid.x>=geometry[0]{return;}
    let point=bitcast<f32>(geometry[1u+2u*gid.x]);
    let carry=geometry[2u+2u*gid.x];
    accumulation[gid.x]=vec4<f32>(f32(aging_scratch_band(point,carry)),f32(aging_floor_mod5120(point)),f32(aging_floor_mod4096(point)),1.0);
}
"#
    );
    let module = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("scratch_band_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut task = Task::new(context.clone(), sphere(white()), request(128), JobOptions::default(), CancelToken::default()).unwrap();
    let bind = context.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 1, resource: input.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: task.accumulation.as_entire_binding() },
        ],
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups((cases.len() as u32).div_ceil(32), 1, 1);
    }
    context.queue.submit([encoder.finish()]);
    task.sample = 1;
    let output = task.read_linear().unwrap();
    for (index, ([point, carry], actual)) in cases.iter().zip(output).enumerate() {
        let point = f64::from(f32::from_bits(*point));
        // Independent real-coordinate arithmetic, not shader cell decomposition.
        // All inputs are within2e11: f64 resolves the represented f32 fractions
        // and the4/5 floor boundaries without the shader's integer algorithm.
        let expected = ((point + 4096.0 * f64::from(*carry)) * 4.0 / 5.0).floor().rem_euclid(4096.0) as f32;
        assert_eq!(actual[0], expected, "band case={index}, point={point}, carry={carry}");
        assert_eq!(actual[1], point.floor().rem_euclid(5120.0) as f32, "cell5120 case={index}, point={point}");
        assert_eq!(actual[2], point.floor().rem_euclid(4096.0) as f32, "cell4096 case={index}, point={point}");
    }
}

#[test]
fn scratch_analytic_triangle_hits_keep_world_phase_for_signed_camera_and_world_origins() {
    use solidcraft_geom::Mesh;
    use solidcraft_render::ProductMesh;
    let Some(executor) = executor() else { return };
    let context = executor.context;
    let source = format!(
        "{SHADER}\n{}",
        r#"
@compute @workgroup_size(32)
fn scratch_geometry_reference(@builtin(global_invocation_id) gid:vec3<u32>){
    if gid.x>=16u{return;}
    let t=triangle(0u);
    let bary=vec3<f32>(1.0-f32(gid.x%4u+1u)/16.0-f32(gid.x/4u+1u)/16.0,f32(gid.x%4u+1u)/16.0,f32(gid.x/4u+1u)/16.0);
    let point=surface_point(t,bary);
    let normal=t.n0.xyz;
    let hit=trace(point+normal*2.0,-normal,false);
    if hit.triangle!=0u {accumulation[gid.x]=vec4<f32>(-1.0);return;}
    let at=surface_point(t,hit.bary);
    let result=aged_surface(t,at,normal);
    accumulation[gid.x]=vec4<f32>(result.base.xyz,1.0);
    accumulation[gid.x+16u]=vec4<f32>(hit.distance,hit.bary.yz,1.0);
    let q=at*u.aging[0].mask.z+u.aging[0].origin.xyz;
    let n=abs(normal);var uv=q.xy;
    let phase_code=u32(u.aging[0].origin.w);var carry=(phase_code/5u)%5u;
    if n.x>n.y && n.x>n.z {uv=q.yz;carry=phase_code/25u;} else if n.y>n.z {uv=q.xz;carry=phase_code/25u;}
    accumulation[gid.x+32u]=vec4<f32>(f32(aging_scratch_band(uv.y,carry)),f32(aging_floor_mod4096(uv.x)),f32(aging_floor_mod4096(uv.y)),1.0);
}
"#
    );
    let module = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &module,
        entry_point: Some("scratch_geometry_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut visible = false;
    for world in [0.0, 999000000.0, -999000000.0] {
        for anchored_origin in [false, true] {
            let origin = if anchored_origin { [world; 3] } else { [0.0; 3] };
            for axis in 0..3 {
                let (u_axis, v_axis) = match axis {
                    0 => (1, 2),
                    1 => (0, 2),
                    _ => (0, 1),
                };
                let mut local = [0.0; 3];
                local[v_axis] = 5.4375;
                let p0 = Vec3::new(world + local[0], world + local[1], world + local[2]);
                let mut along_u = [0.0; 3];
                along_u[u_axis] = 0.25;
                let mut along_v = [0.0; 3];
                along_v[v_axis] = 5.0;
                let u_edge = Vec3::new(along_u[0], along_u[1], along_u[2]);
                let v_edge = Vec3::new(along_v[0], along_v[1], along_v[2]);
                let normal = u_edge.cross(v_edge).normalized().unwrap();
                let mesh = Mesh {
                    positions: vec![p0, p0 + u_edge, p0 + v_edge],
                    normals: vec![normal; 3],
                    triangles: vec![[0, 1, 2]],
                    tri_face: vec![0],
                    ..Default::default()
                };
                let bounds = mesh.bounds();
                let scene = ProductScene {
                    meshes: vec![ProductMesh {
                        name: "Scratch analytic triangle".into(),
                        mesh: Arc::new(mesh),
                        color: [255; 3],
                        opacity: 1.0,
                        surface: Some(white()),
                        transmission: None,
                        faces: Vec::new(),
                    }],
                    bounds,
                };
                let mut stack = AgingStack::default();
                stack
                    .push(AgingLayer {
                        kind: AgingKind::Scratches,
                        scale_mm: 2.0,
                        strength: 1.0,
                        coverage: 1.0,
                        upward_bias: 0.0,
                        color_srgb: [255, 0, 0],
                        origin_mm: origin,
                        ..Default::default()
                    })
                    .unwrap();
                let mut baseline: Option<Vec<[f32; 4]>> = None;
                for cell in [0_i32, -4, -3, -2, -1, 1, 2, 3, 4] {
                    let target =
                        Vec3::new(world + 8192.0 * f64::from(cell), world + 8192.0 * f64::from(2 * cell), world + 8192.0 * f64::from(3 * cell));
                    let mut req = request(8);
                    req.camera.target = target;
                    let mut task =
                        Task::new(context.clone(), scene.clone(), req, JobOptions { aging: stack, ..Default::default() }, CancelToken::default())
                            .unwrap();
                    let parameters = task.parameters(64).unwrap();
                    assert_eq!(parameters.len(), PARAMETER_BYTES);
                    // Thirty-three aligned vec4s precede the aging array. This
                    // asserts actual host serialization, not just gpu_layers().
                    let carry_offset = 33 * 16 + 11 * 4;
                    let carry = f32::from_le_bytes(parameters[carry_offset..carry_offset + 4].try_into().unwrap());
                    let expected = stack.gpu_layers([target.x, target.y, target.z]).unwrap()[0][11];
                    assert_eq!(carry.to_bits(), expected.to_bits());
                    assert_eq!(&parameters[parameters.len() - 4..], &[0; 4], "the determinant materialization lane remains positive zero");
                    context.queue.write_buffer(&task.uniform, 0, &parameters);
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
                    for index in 0..16 {
                        let u = (index % 4 + 1) as f64 / 16.0;
                        let v = (index / 4 + 1) as f64 / 16.0;
                        assert_eq!(
                            pixels[index + 16],
                            [2.0, u as f32, v as f32, 1.0],
                            "analytic hit world={world}, origin={origin:?}, axis={axis}, cell={cell}, index={index}"
                        );
                        let point = p0 + u_edge * u + v_edge * v;
                        let coordinates = [(point.x - origin[0]) / 2.0, (point.y - origin[1]) / 2.0, (point.z - origin[2]) / 2.0];
                        let band = (coordinates[v_axis] * 4.0 / 5.0).floor().rem_euclid(4096.0) as f32;
                        assert_eq!(
                            pixels[index + 32],
                            [band, coordinates[u_axis].floor().rem_euclid(4096.0) as f32, coordinates[v_axis].floor().rem_euclid(4096.0) as f32, 1.0],
                            "independent world phase world={world}, origin={origin:?}, axis={axis}, cell={cell}, index={index}"
                        );
                    }
                    visible |= pixels[..16].iter().any(|pixel| pixel[1] < 0.99);
                    if let Some(reference) = &baseline {
                        assert_eq!(&pixels[..16], reference, "camera-only Scratch change world={world}, origin={origin:?}, axis={axis}, cell={cell}");
                    } else {
                        baseline = Some(pixels[..16].to_vec());
                    }
                }
            }
        }
    }
    assert!(visible, "the analytic world-point fixture must contain visible Scratch coverage");
}

#[test]
fn inactive_scratch_layers_preserve_exact_default_pixels_raw_master_stream_and_admission() {
    let Some(executor) = executor() else { return };
    let options = JobOptions { samples: 4, max_depth: 3, seed: 21026, linear_exr: true, ..Default::default() };
    let baseline = complete_linear_fixture(executor.context.clone(), options);
    let original = admission(&sphere(white()), &request(16), options).unwrap();
    for layer in [
        AgingLayer { kind: AgingKind::Scratches, enabled: false, ..Default::default() },
        AgingLayer { kind: AgingKind::Scratches, strength: 0.0, ..Default::default() },
        AgingLayer { kind: AgingKind::Scratches, coverage: 0.0, ..Default::default() },
    ] {
        let mut aging = AgingStack::default();
        aging.push(layer).unwrap();
        let current = JobOptions { aging, ..options };
        assert_eq!(
            complete_linear_fixture(executor.context.clone(), current),
            baseline,
            "inactive Scratch cannot change PNG/raw bytes or the absolute sample stream"
        );
        let admitted = admission(&sphere(white()), &request(16), current).unwrap();
        assert_eq!(admitted.estimated_total_bytes, original.estimated_total_bytes);
        assert_eq!(admitted.preparation_bytes, original.preparation_bytes);
        assert_eq!(admitted.pixel_scratch_bytes, original.pixel_scratch_bytes);
    }
}

#[test]
fn scratch_keeps_visible_analytic_emission_and_its_raw_master_exact() {
    use solidcraft_render::job::{AreaLight, AreaLights, StudioLighting};
    let Some(executor) = executor() else { return };
    let source = ProductScene { meshes: Vec::new(), bounds: solidcraft_geom::Aabb3::EMPTY };
    let light = AreaLight { center: [0.0; 3], u: [10.0, 0.0, 0.0], v: [0.0, 10.0, 0.0], radiance: [1.5, 0.5, 0.25] };
    let complete = |aging| {
        let options = JobOptions {
            samples: 4,
            max_depth: 2,
            seed: 21026,
            linear_exr: true,
            aging,
            lighting: Some(StudioLighting {
                environment: [0.0; 3],
                sun: None,
                area_lights: AreaLights::new(&[light]).unwrap(),
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut task = Task::new(executor.context.clone(), source.clone(), request(8), options, CancelToken::default()).unwrap();
        for _ in 0..5000 {
            if let TaskPoll::Complete(png) = task.step(16384).unwrap() {
                for pixel in task.read_linear().unwrap() {
                    assert_eq!(pixel, [1.5, 0.5, 0.25, 1.0], "authored analytic-emitter radiance/coverage must remain exact");
                }
                return (png, task.take_linear_exr().unwrap());
            }
        }
        panic!("bounded emission fixture did not complete")
    };
    let baseline = complete(AgingStack::default());
    let mut aging = AgingStack::default();
    aging.push(AgingLayer { kind: AgingKind::Scratches, scale_mm: 2.0, strength: 1.0, coverage: 1.0, ..Default::default() }).unwrap();
    assert_eq!(complete(aging), baseline, "an optical surface layer cannot alter visible analytic emission, PNG/raw identity or its sample stream");
}

#[test]
fn saved_coat_gpu_capture_reopen_and_actual_render_match_independent_normal_incidence_layer() {
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
    use std::f64::consts::{FRAC_PI_2, PI, SQRT_2};
    let Some(executor) = executor() else { return };
    let mut session = solidcraft_engine::Session::default();
    session.execute("solid.box", &json!({"body_name":"Panel","corner":[-10,-10,-2],"length":20,"width":20,"height":2})).unwrap();
    session
        .execute(
            "appearance.assign",
            &json!({"bodies":["Panel"],"name":"Analytic substrate","color":[128,64,32],
        "pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1}}),
        )
        .unwrap();
    let camera = Camera { target: Vec3::ZERO, distance: 1.0, yaw: 0.0, pitch: FRAC_PI_2, roll: 0.0, fov: 0.0, ..Default::default() };
    session
        .execute(
            "render.coat.bind",
            &json!({"name":"Coat","body_index":0,"body_name":"Panel","expected_revision":session.revision,
        "coat":{"weight":0.63,"roughness":0.61,"ior":1.7},"set":{"camera":camera,"width":1,"height":1,"job":{"method":"gpu_path","options":{
            "samples":4,"seed":81,"max_depth":1,"linear_exr":true,"lighting":{"environment":[0,0,0],"sun":null,"area_lights":[],
                "directional_lights":[{"direction":[0,0,1],"color":[1,1,1],"intensity":PI}]}}}}}),
        )
        .unwrap();
    let container = solidcraft_io::write_design_container(&session.doc).unwrap();
    session = solidcraft_engine::Session::new(solidcraft_io::read_design(&container).unwrap());
    session.execute("render.scene", &json!({})).unwrap();
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    session.register_render_job_host(host.clone()).unwrap();
    let document = session.doc.clone();
    let model = session.model.state();
    let evaluation_cache = session.model.cache.clone();
    let world = session.world_state().unwrap();
    let session_camera = session.camera;
    let history = (session.revision, session.model.recomputed, session.undo.len(), session.redo.len());
    let checked = session.execute("render.preflight", &json!({"setup":"Coat"})).unwrap();
    assert_eq!(checked["coats"], json!(session.doc.render.setups["Coat"].coats));
    assert_eq!(checked["resolved"]["executor"], "gpu-path");
    assert_eq!(host.admitted_bytes(), 0);
    let complete = |session: &mut solidcraft_engine::Session, settings: serde_json::Value| {
        let start = session.execute("render.start", &json!({"settings":settings})).unwrap();
        let id: JobId = serde_json::from_value(start["job"].clone()).unwrap();
        let handle = session.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{status:?}");
        handle.frame().unwrap().unwrap()
    };
    let saved = session.doc.render.setups["Coat"].clone();
    let coated = complete(&mut session, json!(saved));
    let raw = raw_pixel(coated.linear_exr().unwrap());
    assert_eq!(raw[3], 1.0);
    // Independent normal-incidence dielectric GGX coat and its documented
    // conservative entry/exit attenuation, over an IOR1 Lambert substrate.
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
        assert!((f64::from(*actual) - expected).abs() < 2e-6, "GPU coat radiance {actual} vs independent {expected}");
    }
    let mut plain = saved.clone();
    plain.coats = Default::default();
    let baseline_check = session.execute("render.preflight", &json!({"settings":plain})).unwrap();
    assert_eq!(checked["admission"], baseline_check["admission"]);
    assert_eq!(checked["retained_options_bytes"], baseline_check["retained_options_bytes"]);
    let mut zero = saved;
    let row = zero.coats.iter().next().unwrap().clone();
    zero.coats
        .set(
            solidcraft_engine::doc::render_coat::SavedCoat {
                coat: solidcraft_engine::doc::render_coat::CoatMaterial { weight: 0.0, ..row.coat },
                ..row
            },
            true,
        )
        .unwrap();
    let baseline = complete(&mut session, json!(plain));
    let identity = complete(&mut session, json!(zero));
    assert_eq!(baseline.png(), identity.png());
    assert_eq!(baseline.linear_exr().unwrap(), identity.linear_exr().unwrap());
    assert_ne!(coated.linear_exr().unwrap(), baseline.linear_exr().unwrap());
    assert!(Arc::ptr_eq(&document, &session.doc));
    assert!(Arc::ptr_eq(&model, &session.model.state()));
    assert!(Arc::ptr_eq(&evaluation_cache, &session.model.cache));
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    assert_eq!(session.camera, session_camera);
    assert_eq!((session.revision, session.model.recomputed, session.undo.len(), session.redo.len()), history);
}

#[test]
fn saved_gpu_coat_survives_builtin_maps_aging_and_guided_filter_without_changing_the_raw_master() {
    let Some(executor) = executor() else { return };
    let mut session = solidcraft_engine::Session::default();
    session.execute("solid.box", &json!({"body_name":"Panel","corner":[-10,-10,-2],"length":20,"width":20,"height":2})).unwrap();
    session
        .execute("appearance.assign", &json!({"bodies":["Panel"],"color":[128,64,32],"pbr":{"metallic":0,"roughness":0.5,"dielectric_ior":1}}))
        .unwrap();
    let camera = Camera { distance: 10.0, yaw: 0.0, pitch: std::f64::consts::FRAC_PI_2, roll: 0.0, fov: 0.0, ..Default::default() };
    session.execute("render.coat.bind",&json!({"name":"Coat","body_index":0,"body_name":"Panel","expected_revision":session.revision,
        "coat":{"weight":0.63,"roughness":0.61,"ior":1.7},"set":{"camera":camera,"width":16,"height":16,"job":{"method":"gpu_path","options":{
            "samples":4,"seed":81,"max_depth":2,"linear_exr":true,"denoise":true,"lighting":{"environment":[0,0,0],"sun":null,"area_lights":[],
                "directional_lights":[{"direction":[0,0,1],"color":[1,1,1],"intensity":3}]},"aging":[{"kind":"dust","strength":0.2,"coverage":1}]}}}})).unwrap();
    session
        .execute("render.library.surface.bind", &json!({"name":"Coat","surface":{"id":"SURF-001","body_index":0,"body_name":"Panel","scale":10}}))
        .unwrap();
    let bytes = solidcraft_io::write_design_container(&session.doc).unwrap();
    session = solidcraft_engine::Session::new(solidcraft_io::read_design(&bytes).unwrap());
    session.execute("render.scene", &json!({})).unwrap();
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    session.register_render_job_host(host).unwrap();
    let document = session.doc.clone();
    let model = session.model.state();
    let evaluation_cache = session.model.cache.clone();
    let world = session.world_state().unwrap();
    let session_camera = session.camera;
    let history = (session.revision, session.model.recomputed, session.undo.len(), session.redo.len());
    let complete = |session: &mut solidcraft_engine::Session, setup: solidcraft_engine::doc::render_settings::RenderSetup| {
        let reply = session.execute("render.start", &json!({"settings":setup})).unwrap();
        let id: JobId = serde_json::from_value(reply["job"].clone()).unwrap();
        let handle = session.render_job_handle(id).unwrap();
        let status = handle.wait_timeout(Duration::from_secs(60)).unwrap();
        assert_eq!(status.progress.phase, Phase::Completed, "{status:?}");
        handle.frame().unwrap().unwrap()
    };
    let saved = session.doc.render.setups["Coat"].clone();
    let actual = complete(&mut session, saved.clone());
    let mut uncoated = saved.clone();
    uncoated.coats = Default::default();
    let plain = complete(&mut session, uncoated);
    assert_ne!(actual.linear_exr().unwrap(), plain.linear_exr().unwrap(), "maps and aging cannot erase the captured coat");
    let mut unmapped = saved.clone();
    unmapped.surfaces = Default::default();
    let no_map = complete(&mut session, unmapped);
    assert_ne!(actual.linear_exr().unwrap(), no_map.linear_exr().unwrap(), "maps still modify the coated substrate");
    let mut unaged = saved.clone();
    let Some(solidcraft_engine::doc::render_settings::RenderJob::GpuPath { options, .. }) = &mut unaged.job else { panic!("GPU") };
    options.aging = Default::default();
    let no_age = complete(&mut session, unaged);
    assert_ne!(actual.linear_exr().unwrap(), no_age.linear_exr().unwrap(), "aging still modifies the mapped coat/substrate");
    let mut unfiltered = saved.clone();
    let Some(solidcraft_engine::doc::render_settings::RenderJob::GpuPath { options, .. }) = &mut unfiltered.job else { panic!("GPU") };
    options.denoise = false;
    let raw = complete(&mut session, unfiltered);
    assert_eq!(actual.linear_exr().unwrap(), raw.linear_exr().unwrap(), "denoising cannot enter coated raw transport");
    assert_eq!(session.doc.render.setups["Coat"], saved);
    assert!(Arc::ptr_eq(&document, &session.doc));
    assert!(Arc::ptr_eq(&model, &session.model.state()));
    assert!(Arc::ptr_eq(&evaluation_cache, &session.model.cache));
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    assert_eq!(session.camera, session_camera);
    assert_eq!((session.revision, session.model.recomputed, session.undo.len(), session.redo.len()), history);
}
