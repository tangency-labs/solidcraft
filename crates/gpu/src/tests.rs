use solidcraft_geom::Vec3;
use solidcraft_kernel::{Body, BoolOp};

use super::*;

/// The GPU, or `None` with a note when the machine has no adapter (CI runners without a GPU).
/// A device that cannot build the pipeline fails the test: that is a shader the device rejects
/// (as D3D12's FXC once did), not a missing GPU. `SOLIDCRAFT_REQUIRE_GPU=1` (the Windows CI job,
/// on the WARP software adapter) fails without an adapter too.
fn gpu() -> Option<TessGpu> {
    match TessGpu::headless() {
        Ok(g) => {
            eprintln!("GPU: {}", g.adapter_name());
            Some(g)
        }
        Err(e @ GpuError::Pipeline(_)) => panic!("{e}"),
        Err(e) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_some() => panic!("a GPU is required here: {e}"),
        Err(e) => {
            eprintln!("skipping the GPU comparison: {e}");
            None
        }
    }
}

/// A shader the device rejects when the pipeline is created (its binding 1 is writable, the
/// layout's read-only), standing in for one a device's compiler cannot build.
const REJECTED: &str = "
@group(0) @binding(1) var<storage, read_write> surfaces: array<u32>;
@group(0) @binding(4) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    surfaces[gid.x] = 1u;
    out[gid.x] = vec4<f32>(0.0);
}
";

/// A pipeline the device cannot build is an error the caller can fall back from (to the CPU
/// mesh), not a panic in the device's uncaptured-error handler; and the device stays usable.
#[test]
fn a_pipeline_the_device_rejects_is_an_error_not_a_crash() {
    let (adapter, device, queue) = match open_headless() {
        Ok(d) => d,
        Err(e) => {
            assert!(std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "a GPU is required here: {e}");
            eprintln!("skipping: {e}");
            return;
        }
    };
    let name = adapter.get_info().name;
    match TessGpu::with_shader(name.clone(), device.clone(), queue.clone(), REJECTED) {
        Err(GpuError::Pipeline(e)) => eprintln!("rejected as expected: {e}"),
        Err(e) => panic!("unexpected error: {e}"),
        Ok(_) => panic!("the rejected shader built a pipeline"),
    }
    // Not wgsl at all: also an error, not a panic.
    assert!(matches!(TessGpu::with_shader(name.clone(), device.clone(), queue.clone(), "fn main( {"), Err(GpuError::Pipeline(_))));
    // The same device then builds the real pipeline and evaluates on it.
    let gpu = ok(TessGpu::new(name, device, queue));
    let body = ok(solidcraft_kernel::torus(Vec3::ZERO, 6.0, 1.5));
    let plan = ok(body.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.01)));
    assert_eq!(ok(gpu.evaluate(&plan)).positions.len(), plan.evaluate().positions.len());
}

/// The shader as the D3D12 backend translates it (naga's HLSL for shader model 5.1, the FXC
/// target, with wgpu's options) avoids what FXC cannot compile without unrolling a loop:
/// arrays inside structs and two-dimensional arrays written by a loop counter. Runs everywhere;
/// the Windows CI job compiles the real thing with FXC.
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

fn ok<T, E: std::fmt::Debug>(r: std::result::Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e:?}"),
    }
}

fn bodies() -> Vec<(&'static str, Body)> {
    let bx = ok(solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)));
    let pin = ok(solidcraft_kernel::cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.0, 6.0));
    let cross = ok(solidcraft_kernel::cylinder(Vec3::new(5.0, -1.0, 2.0), Vec3::Y, 1.0, 12.0));
    let cut = |a: &Body, b: &Body| ok(solidcraft_kernel::boolean(a, b, BoolOp::Cut)).unwrap_or_else(|| a.clone());
    let edges = [Vec3::new(5.0, 0.0, 4.0), Vec3::new(10.0, 5.0, 4.0), Vec3::new(5.0, 10.0, 4.0), Vec3::new(0.0, 5.0, 4.0)];
    vec![
        ("box with a hole", cut(&bx, &pin)),
        ("cross hole", cut(&pin, &cross)),
        ("torus", ok(solidcraft_kernel::torus(Vec3::ZERO, 6.0, 1.5))),
        ("fillets", ok(solidcraft_kernel::fillet(&bx, &edges, 1.5))),
        (
            "mirrored",
            ok(solidcraft_kernel::transform_matrix(&pin, [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [3.0, 0.0, 1.0, 1.0]])),
        ),
    ]
}

#[test]
fn gpu_evaluation_matches_the_cpu() {
    let Some(gpu) = gpu() else { return };
    for (name, body) in bodies() {
        let plan = ok(body.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.01)));
        let cpu = plan.evaluate();
        let got = ok(gpu.evaluate(&plan));
        assert_eq!(got.triangles, cpu.triangles, "{name}");
        assert_eq!(got.tri_face, cpu.tri_face, "{name}");
        assert_eq!(got.positions.len(), cpu.positions.len(), "{name}");
        let size = 20.0;
        for (k, (p, q)) in got.positions.iter().zip(&cpu.positions).enumerate() {
            assert!(p.dist(*q) < size * 1e-5, "{name}: vertex {k} at {p:?}, CPU {q:?}");
        }
        for (k, (n, m)) in got.normals.iter().zip(&cpu.normals).enumerate() {
            assert!(n.dist(*m) < 2e-3, "{name}: normal {k} {n:?}, CPU {m:?}");
        }
        // Fixed edge nodes are exact: shared boundaries are bit-identical to the CPU mesh's.
        let mut k = 0;
        for f in &plan.faces {
            for (j, p) in f.fixed.iter().enumerate() {
                assert_eq!(got.positions.get(k + j), Some(p), "{name}");
            }
            k += f.uv.len();
        }
    }
}

/// A plan drawn straight from the GPU: `prepare` and `record` into an encoder, as the viewport
/// does, give the vertices and triangles the CPU evaluator gives (read back here only to check).
#[test]
fn a_resident_mesh_matches_the_cpu() {
    let Some(gpu) = gpu() else { return };
    for (name, body) in bodies() {
        let plan = ok(body.mesh_plan(&solidcraft_kernel::MeshTolerance { chord: 0.01, angle: Some(std::f64::consts::PI / 9.0) }));
        let cpu = plan.evaluate();
        let mesh = ok(gpu.prepare(&plan));
        assert_eq!(mesh.vertex_count as usize, cpu.positions.len(), "{name}");
        assert_eq!(mesh.index_count as usize, cpu.triangles.len() * 3, "{name}");
        assert_eq!(mesh.vertex_face.len(), cpu.positions.len(), "{name}");
        for (t, f) in cpu.triangles.iter().zip(&cpu.tri_face) {
            assert!(t.iter().all(|&i| mesh.vertex_face[i as usize] == *f), "{name}: a triangle's vertices belong to its face");
        }
        let d = &gpu.device;
        let read = |src: &wgpu::Buffer, size: u64| {
            let dst = d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("test read"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut enc = d.create_command_encoder(&Default::default());
            gpu.record(&mut enc, &mesh);
            enc.copy_buffer_to_buffer(src, 0, &dst, 0, size);
            gpu.queue.submit([enc.finish()]);
            let slice = dst.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            ok(d.poll(wgpu::PollType::wait_indefinitely()));
            let bytes = ok(slice.get_mapped_range());
            bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect::<Vec<f32>>()
        };
        let out = read(&mesh.vertices, u64::from(mesh.vertex_count) * VERTEX_STRIDE);
        for (k, (p, n)) in cpu.positions.iter().zip(&cpu.normals).enumerate() {
            let at = |i: usize| f64::from(out[8 * k + i]);
            assert!(Vec3::new(at(0), at(1), at(2)).dist(*p) < 20.0 * 1e-5, "{name}: vertex {k}");
            assert!(Vec3::new(at(4), at(5), at(6)).dist(*n) < 2e-3, "{name}: normal {k}");
        }
    }
}

/// Several bodies in one buffer and one dispatch (a viewport's scene): each plan's part holds
/// exactly its own vertices and triangles, evaluated as the CPU evaluates that plan alone.
#[test]
fn a_scene_in_one_dispatch_matches_each_body_on_the_cpu() {
    let Some(gpu) = gpu() else { return };
    let plans: Vec<TessPlan> = bodies().iter().map(|(_, b)| ok(b.mesh_plan(&solidcraft_kernel::MeshTolerance { chord: 0.01, angle: Some(std::f64::consts::PI / 9.0) }))).collect();
    let refs: Vec<&TessPlan> = plans.iter().collect();
    let mesh = ok(gpu.prepare_many(&refs));
    assert_eq!(mesh.parts.len(), plans.len());
    let d = &gpu.device;
    let size = u64::from(mesh.vertex_count) * VERTEX_STRIDE;
    let dst = d.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test read"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = d.create_command_encoder(&Default::default());
    gpu.record(&mut enc, &mesh);
    enc.copy_buffer_to_buffer(&mesh.vertices, 0, &dst, 0, size);
    gpu.queue.submit([enc.finish()]);
    let slice = dst.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    ok(d.poll(wgpu::PollType::wait_indefinitely()));
    let out: Vec<f32> = ok(slice.get_mapped_range()).as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    for ((plan, part), (name, _)) in plans.iter().zip(&mesh.parts).zip(bodies()) {
        let cpu = plan.evaluate();
        assert_eq!((part.vertices.end - part.vertices.start) as usize, cpu.positions.len(), "{name}");
        assert_eq!((part.indices.end - part.indices.start) as usize, cpu.triangles.len() * 3, "{name}");
        for (k, p) in cpu.positions.iter().enumerate() {
            let i = part.vertices.start as usize + k;
            let q = Vec3::new(f64::from(out[8 * i]), f64::from(out[8 * i + 1]), f64::from(out[8 * i + 2]));
            assert!(q.dist(*p) < 20.0 * 1e-5, "{name}: vertex {k}");
        }
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
        surfaces: vec![EvalSurface::Plane { origin: Vec3::ZERO, du: Vec3::X, dv: Vec3::Y }],
        faces: vec![solidcraft_geom::tess::FacePlan { surface: Some(0), ..Default::default() }],
        ..Default::default()
    };
    check_empty_tessellation(&gpu, &plan);
}

#[test]
fn tessellation_without_surfaces_does_not_dispatch() {
    let Some(gpu) = gpu() else { return };
    check_empty_tessellation(&gpu, &TessPlan::default());
}

/// Display meshing of a 400-hole plate (#303's benchmark): reference mesher, plan, CPU and GPU
/// evaluation. `cargo test --release -p solidcraft-gpu bench_plate -- --ignored --nocapture`.
#[test]
#[ignore]
fn bench_plate() {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let rect = |w: f64| Loop2 {
        segs: vec![
            Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(w, 0.0) },
            Seg2::Line { a: Vec2::new(w, 0.0), b: Vec2::new(w, w) },
            Seg2::Line { a: Vec2::new(w, w), b: Vec2::new(0.0, w) },
            Seg2::Line { a: Vec2::new(0.0, w), b: Vec2::new(0.0, 0.0) },
        ],
    };
    let holes: Vec<Loop2> = (0..400)
        .map(|k| {
            let c = Vec2::new(5.0 + 10.0 * (k % 20) as f64, 5.0 + 10.0 * (k / 20) as f64);
            Loop2 { segs: vec![Seg2::Arc { center: c, radius: 3.0, start: 0.0, sweep: -std::f64::consts::TAU }] }
        })
        .collect();
    let plate = ok(solidcraft_kernel::extrude(&Plane::XY, &[Region2 { outer: rect(200.0), holes }], 0.0, 5.0)).remove(0);
    let gpu = gpu();
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1e3;
    let median = |mut v: Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    println!("faces {}", plate.face_count());
    println!("| tol | reference | plan | CPU eval | GPU eval | triangles (ref / plan) |");
    for tol in [0.3, 0.03, 0.006] {
        let (mut r, mut p, mut c, mut g) = (vec![], vec![], vec![], vec![]);
        let (mut nr, mut np) = (0, 0);
        for _ in 0..5 {
            let t = std::time::Instant::now();
            let m = ok(plate.mesh(&solidcraft_kernel::MeshTolerance::chord(tol)));
            r.push(ms(t));
            nr = m.triangles.len();
            let t = std::time::Instant::now();
            let plan = ok(plate.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(tol)));
            p.push(ms(t));
            let t = std::time::Instant::now();
            let m = plan.evaluate();
            c.push(ms(t));
            np = m.triangles.len();
            if let Some(gpu) = &gpu {
                let t = std::time::Instant::now();
                let _ = ok(gpu.evaluate(&plan));
                g.push(ms(t));
            }
        }
        let g = if g.is_empty() { "-".to_string() } else { format!("{:.1} ms", median(g)) };
        println!("| {tol} | {:.1} ms | {:.1} ms | {:.1} ms | {g} | {nr} / {np} |", median(r), median(p), median(c));
    }
}
