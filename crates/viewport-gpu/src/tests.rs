use solidcore_record::Vec3;

use super::*;

/// The GPU, or `None` with a note when the machine has no adapter (CI runners without a GPU),
/// unless a GPU is required ([`test_env`]). A device that cannot build the pipeline fails the
/// test: that is a shader the device rejects (as D3D12's FXC once did), not a missing GPU.
fn gpu() -> Option<TessGpu> {
    match TessGpu::headless(test_env::policy()) {
        Ok(g) => {
            eprintln!("GPU: {}", g.adapter_name());
            Some(g)
        }
        Err(e @ GpuError::Pipeline(_)) => panic!("{e}"),
        Err(e) => {
            test_env::skip_unless_required(test_env::required(), &e);
            None
        }
    }
}

fn ok<T, E: std::fmt::Debug>(r: std::result::Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e:?}"),
    }
}

/// Several scopes catch errors: running out of memory is reported in preference to the validation
/// errors a refused buffer causes (#4791), and `buffer_init` pads like `create_buffer_init`.
#[test]
fn out_of_memory_is_reported_before_secondary_validation_errors() {
    let validation = || wgpu::Error::Validation { source: "invalid buffer".into(), description: "invalid buffer".into() };
    let oom = || wgpu::Error::OutOfMemory { source: "refused".into() };
    let picked = preferred_error([validation(), oom()]).map(|e| describe_error(&e));
    assert_eq!(picked.as_deref(), Some("GPU out of memory"));
    let picked = preferred_error([validation()]).map(|e| describe_error(&e));
    assert!(picked.is_some_and(|m| m.contains("invalid buffer")));
    assert!(preferred_error([]).is_none());
    let Ok((_, device, _)) = open_headless(test_env::policy()) else { return };
    let buffer = buffer_init(&device, "test", &[1, 2, 3], wgpu::BufferUsages::VERTEX);
    assert_eq!(buffer.map(|b| b.size()).map_err(|e| e.to_string()), Ok(wgpu::COPY_BUFFER_ALIGNMENT));
}

/// [`buffer_write`]'s buffer holds what it was given, padded with zeros to whole words as
/// [`buffer_init`]'s is, in the command buffers submitted after it.
#[test]
fn a_buffer_written_through_the_queue_holds_its_contents_padded() {
    let Ok((_, device, queue)) = open_headless(test_env::policy()) else { return };
    for contents in [&[][..], &[7][..], &[1, 2, 3, 4][..], &[1, 2, 3, 4, 5, 6][..]] {
        let buffer = ok(buffer_write(&device, &queue, "test", contents, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_SRC));
        let mut want = contents.to_vec();
        want.resize(contents.len().div_ceil(4).max(1) * 4, 0);
        assert_eq!(buffer.size(), want.len() as u64);
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: buffer.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&buffer, 0, &read, 0, buffer.size());
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        assert_eq!(ok(read.slice(..).get_mapped_range()).to_vec(), want, "{contents:?}");
    }
}

/// A buffer the device does not make is an error, as [`buffer_init`]'s is, whatever it was to hold:
/// the scene upload then keeps the previous picture.
#[test]
fn a_buffer_the_device_refuses_is_an_error_when_written() {
    let Ok((_, device, queue)) = open_headless(test_env::policy()) else { return };
    let limit = device.limits().max_buffer_size;
    // Only where asking for too much takes little memory here.
    let Some(size) = usize::try_from(limit).ok().filter(|l| *l <= 512 << 20).and_then(|l| l.checked_add(4)) else { return };
    let contents = vec![0; size];
    assert!(buffer_write(&device, &queue, "test", &contents, wgpu::BufferUsages::VERTEX).is_err());
    // The device goes on making buffers.
    assert!(buffer_write(&device, &queue, "test", &[1, 2, 3, 4], wgpu::BufferUsages::VERTEX).is_ok());
}

#[test]
fn the_hlsl_for_fxc_has_no_arrays_fxc_must_unroll_loops_for() {
    use naga::back::hlsl;
    let module = ok(naga::front::wgsl::parse_str(SHADER));
    let info = ok(naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::default()).validate(&module));
    let options = hlsl::Options {
        shader_model: hlsl::ShaderModel::V5_1,
        force_loop_bounding: true,
        restrict_indexing: true,
        fake_missing_bindings: true,
        ..Default::default()
    };
    let pipeline = hlsl::PipelineOptions::default();
    let mut out = String::new();
    ok(hlsl::Writer::new(&mut out, &options, &pipeline).write(&module, &info, None));
    assert!(out.contains("void main("), "{out}");
    // Struct members that are arrays: `float n0_[8];` inside a `struct ... { };`.
    let mut in_struct = false;
    for line in out.lines() {
        let l = line.trim();
        if l.starts_with("struct ") && !l.ends_with(';') {
            in_struct = true;
        } else if l.starts_with("};") {
            in_struct = false;
        } else if in_struct {
            assert!(!l.contains('['), "an array inside a struct: {l}");
        }
        assert!(!l.contains("]["), "a two-dimensional array: {l}");
    }
}

fn check_empty_tessellation(gpu: &TessGpu, plan: &TessPlan) {
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let evaluated = ok(gpu.evaluate(plan));
    assert!(evaluated.positions.is_empty());
    assert!(evaluated.normals.is_empty());
    assert!(evaluated.triangles.is_empty());

    let mesh = ok(gpu.prepare(plan));
    assert_eq!(mesh.vertex_count, 0);
    assert_eq!(mesh.index_count, 0);
    assert!(mesh._inputs[1].size() >= std::mem::size_of::<[u32; SURFACE_WORDS]>() as u64);
    assert!(mesh._inputs[2].size() >= std::mem::size_of::<f32>() as u64);
    assert!(mesh._inputs[3].size() >= std::mem::size_of::<[u32; VERTEX_WORDS]>() as u64);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    gpu.record(&mut encoder, &mesh);
    gpu.queue.submit([encoder.finish()]);
    ok(gpu.device.poll(wgpu::PollType::wait_indefinitely()));
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "{error:?}");
}

#[test]
fn tessellation_without_vertices_does_not_dispatch() {
    let Some(gpu) = gpu() else { return };
    let plan = TessPlan {
        surfaces: vec![EvalSurface::Plane {
            origin: solidcore_tess::vec3(Vec3::ZERO),
            du: solidcore_tess::vec3(Vec3::X),
            dv: solidcore_tess::vec3(Vec3::Y),
        }],
        faces: vec![solidcore_tess::FacePlan { surface: Some(0), ..Default::default() }],
        ..Default::default()
    };
    check_empty_tessellation(&gpu, &plan);
}

#[test]
fn tessellation_without_surfaces_does_not_dispatch() {
    let Some(gpu) = gpu() else { return };
    check_empty_tessellation(&gpu, &TessPlan::default());
}

#[test]
fn plan_validation_matches_the_packing_refusals() {
    let plan = TessPlan {
        surfaces: vec![EvalSurface::Plane {
            origin: solidcore_tess::vec3(Vec3::ZERO),
            du: solidcore_tess::vec3(Vec3::X),
            dv: solidcore_tess::vec3(Vec3::Y),
        }],
        faces: vec![solidcore_tess::FacePlan { face: 7, surface: Some(0), ..Default::default() }],
        ..Default::default()
    };
    assert!(validate_plan(&plan).is_ok());
    assert!(PackedPlans::of(&[&plan]).is_ok());
    for reason in ["pole", "fixed", "missing surface"] {
        let mut refused = plan.clone();
        let face = &mut refused.faces[0];
        match reason {
            "pole" => face.poles.push(solidcore_tess::PoleNode { node: 0, rule: solidcore_tess::PoleRule::Averaged }),
            "fixed" => face.surface = None,
            _ => face.surface = Some(1),
        }
        let check = validate_plan(&refused).unwrap_err();
        let packed = PackedPlans::of(&[&refused]).unwrap_err();
        assert_eq!(check.to_string(), packed.to_string(), "{reason}");
        match reason {
            "pole" => assert!(matches!(check, GpuError::PoleNodes { face: 7 })),
            "fixed" => assert!(matches!(check, GpuError::Unsupported(_))),
            _ => assert!(matches!(check, GpuError::Failed(_))),
        }
    }
}
