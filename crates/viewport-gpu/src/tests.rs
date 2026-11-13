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
fn a_regular_nurbs_patch_keeps_its_normal_with_nearly_parallel_tangents() {
    let Some(gpu) = gpu() else { return };
    let mut plan = TessPlan::default();
    for rise in [5e-7, 1e-24] {
        let surface = plan.surfaces.len() as u32;
        plan.surfaces.push(EvalSurface::nurbs(
            [1, 1],
            [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
            [2, 2],
            vec![[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [1.0, 0.0, rise, 1.0], [2.0, 0.0, rise, 1.0]],
        ));
        plan.faces.push(solidcore_tess::FacePlan {
            face: surface,
            surface: Some(surface),
            uv: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
            ..Default::default()
        });
    }
    let mesh = ok(gpu.evaluate(&plan));
    assert_eq!(mesh.normals.len(), 6);
    for normal in mesh.normals {
        assert!(normal.x.abs() < 1e-5 && (normal.y + 1.0).abs() < 1e-5 && normal.z.abs() < 1e-5, "{normal:?}");
    }
}
