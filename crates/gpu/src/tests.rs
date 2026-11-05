use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
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

/// The shader's basis slots hold the highest degree the CPU evaluator takes, so every surface
/// the CPU evaluates the GPU evaluates too.
#[test]
fn the_shader_takes_every_degree_the_cpu_evaluator_does() {
    let slots = format!("const W: u32 = {}u;", solidcraft_geom::bspline::MAX_SPLINE_DEGREE + 1);
    assert!(SHADER.contains(&slots), "tess.wgsl must declare `{slots}`");
}

/// Browsers compile WGSL with Tint, which keeps a rule of the WGSL grammar that naga doesn't: a
/// shift, `&`, `|` and `^` don't mix in one expression without parentheses (`a << 3u | b` is
/// refused, `(a << 3u) | b` isn't). A shader that breaks it runs on the desktop and leaves the
/// web app's 3D view black (#11080). Every shader in the repository keeps to it.
#[test]
fn no_shader_mixes_shifts_and_bitwise_operators_without_parentheses() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut shaders = Vec::new();
    let mut dirs = vec![root.join("assets"), root.join("crates"), root.join("vendor")];
    while let Some(dir) = dirs.pop() {
        for entry in ok(std::fs::read_dir(&dir)) {
            let path = ok(entry).path();
            if path.is_dir() {
                if !path.ends_with("target") {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "wgsl") {
                shaders.push(path);
            }
        }
    }
    assert!(shaders.iter().any(|p| p.ends_with("assets/shaders/viewport.wgsl")), "{shaders:?}");
    let mut found = Vec::new();
    for path in &shaders {
        for (n, line) in ok(std::fs::read_to_string(path)).lines().enumerate() {
            if let Some(ops) = mixed_bitwise(line) {
                found.push(format!("{}:{}: {ops}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
    // The check finds the line #11080 reported, and takes it parenthesized.
    assert_eq!(mixed_bitwise("let v = ((x ^ y) & 1u) << 3u | (y & 1u) << 2u;").as_deref(), Some("shift and |"));
    assert_eq!(mixed_bitwise("let v = (((x ^ y) & 1u) << 3u) | ((y & 1u) << 2u);"), None);
    assert_eq!(mixed_bitwise("let a = f(&p, x & 1u, y | 2u) && b || c; // x << 1u | y"), None);
    assert_eq!(mixed_bitwise("var<storage> m: array<vec2<f32>>; let b = a & 1u ^ c;").as_deref(), Some("& and ^"));
}

/// The bitwise operators (a shift, `&`, `|`, `^`) that one parenthesized group of a WGSL line
/// mixes, when it mixes more than one: what WGSL needs parentheses for. `&&`, `||`, an
/// address-of `&` and a `>>` closing two generic brackets (`vec2<f32>>`) aren't counted.
fn mixed_bitwise(line: &str) -> Option<String> {
    let chars: Vec<char> = line.split("//").next().unwrap_or("").chars().collect();
    let at = |i: usize| chars.get(i).copied().unwrap_or(' ');
    let mixed = |g: &Vec<&str>| (g.len() > 1).then(|| g.join(" and "));
    let mut groups: Vec<Vec<&str>> = vec![Vec::new()];
    let (mut i, mut prev) = (0, ' ');
    while i < chars.len() {
        let (c, next) = (at(i), at(i + 1));
        let mut op = None;
        match c {
            '(' | '[' => groups.push(Vec::new()),
            ')' | ']' | ',' | ';' | '{' | '}' => {
                if let Some(m) = groups.last().and_then(mixed) {
                    return Some(m);
                }
                if matches!(c, ')' | ']') && groups.len() > 1 {
                    groups.pop();
                } else if let Some(g) = groups.last_mut() {
                    g.clear();
                }
            }
            '<' if next == '<' => {
                op = Some("shift");
                i += 1;
            }
            '>' if next == '>' => {
                let after = at(i + 2);
                if after == ' ' || after == '_' || after.is_alphanumeric() {
                    op = Some("shift");
                }
                i += 1;
            }
            '&' | '|' if next == c => i += 1,
            '&' if prev == '_' || prev == ')' || prev == ']' || prev.is_alphanumeric() => op = Some("&"),
            '|' => op = Some("|"),
            '^' => op = Some("^"),
            _ => {}
        }
        if let (Some(op), Some(g)) = (op, groups.last_mut())
            && !g.contains(&op)
        {
            g.push(op);
        }
        if !at(i).is_whitespace() {
            prev = at(i);
        }
        i += 1;
    }
    groups.iter().find_map(mixed)
}

/// Unwraps a `Result` in test code, panicking with its `Debug` representation on `Err`.
fn ok<T, E: std::fmt::Debug>(r: std::result::Result<T, E>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e:?}"),
    }
}

/// Builds a representative suite of B-rep test solids (booleans, torus, fillets, mirror transform).
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

/// Verifies that GPU tessellation evaluation matches the CPU reference evaluator across all test bodies.
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
    let plans: Vec<TessPlan> = bodies()
        .iter()
        .map(|(_, b)| ok(b.mesh_plan(&solidcraft_kernel::MeshTolerance { chord: 0.01, angle: Some(std::f64::consts::PI / 9.0) })))
        .collect();
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

/// Asserts that evaluating and preparing an empty or zero-vertex [`TessPlan`] succeeds without validation errors.
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

/// Verifies that a [`TessPlan`] with a surface but zero vertices skips GPU compute dispatch cleanly.
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

/// Verifies that an empty default [`TessPlan`] with zero surfaces and zero vertices skips GPU compute dispatch cleanly.
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
    use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2};
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

/// Verifies numerical parity of `f32` and `f64` bicubic Bézier tensor-product patch evaluation across CPU, Vulkan, CUDA PTX, and Ampere DMMA Tensor Cores.
#[test]
fn multi_backend_bicubic_patch_f32_and_f64_dmma_parity() {
    use crate::cuda::{CudaBackend, PatchUvQueryF32};
    use crate::dispatch::{
        VulkanAccel, cpu_eval_bicubic_f32_rayon, cpu_eval_bicubic_f32_scalar, cpu_eval_bicubic_f64_rayon, cpu_eval_bicubic_f64_scalar,
        synthetic_bicubic_patch_f32, synthetic_bicubic_patch_f64,
    };

    let patch_f32 = synthetic_bicubic_patch_f32();
    let patch_f64 = synthetic_bicubic_patch_f64();

    // Use 259 queries (non-multiple of 4 and 64) to exercise tail handling in DMMA and workgroups.
    let n = 259usize;
    let queries_f32: Vec<PatchUvQueryF32> = (0..n)
        .map(|i| {
            let t = (i as f32) / (n as f32);
            PatchUvQueryF32 { u: (t * 0.89 + 0.05).fract(), v: (t * 2.71 + 0.07).fract(), patch_idx: 0, pad: 0 }
        })
        .collect();
    let queries_f64: Vec<[f64; 4]> = queries_f32.iter().map(|q| [q.u as f64, q.v as f64, 0.0, 0.0]).collect();

    let ref_f32 = cpu_eval_bicubic_f32_scalar(&patch_f32, &queries_f32);
    let ray_f32 = cpu_eval_bicubic_f32_rayon(&patch_f32, &queries_f32);
    assert_eq!(ref_f32, ray_f32);

    if let Some(vk) = VulkanAccel::global() {
        let vk_out = vk.eval_bicubic_f32(&patch_f32, &queries_f32).expect("vk eval_bicubic_f32");
        assert_eq!(vk_out.len(), ref_f32.len());
        for (i, (a, b)) in ref_f32.iter().zip(&vk_out).enumerate() {
            for c in 0..3 {
                assert!((a.pos[c] - b.pos[c]).abs() < 1e-4, "vk pos mismatch at {i}[{c}]: {:?} vs {:?}", a.pos, b.pos);
                assert!((a.norm[c] - b.norm[c]).abs() < 1e-4, "vk norm mismatch at {i}[{c}]: {:?} vs {:?}", a.norm, b.norm);
            }
        }
    }

    let ref_f64 = cpu_eval_bicubic_f64_scalar(&patch_f64, &queries_f64);
    let ray_f64 = cpu_eval_bicubic_f64_rayon(&patch_f64, &queries_f64);
    assert_eq!(ref_f64, ray_f64);

    if let Some(cuda) = CudaBackend::global() {
        let cu_f32_gen = cuda.eval_bicubic_f32(&patch_f32, &queries_f32, false).expect("cuda generic f32");
        for (i, (a, b)) in ref_f32.iter().zip(&cu_f32_gen).enumerate() {
            for c in 0..3 {
                assert!((a.pos[c] - b.pos[c]).abs() < 1e-4, "cuda sm70 f32 pos mismatch at {i}[{c}]");
                assert!((a.norm[c] - b.norm[c]).abs() < 1e-4, "cuda sm70 f32 norm mismatch at {i}[{c}]");
            }
        }
        if cuda.has_ampere_ptx() {
            let cu_f32_amp = cuda.eval_bicubic_f32(&patch_f32, &queries_f32, true).expect("cuda sm86 cp.async f32");
            for (i, (a, b)) in ref_f32.iter().zip(&cu_f32_amp).enumerate() {
                for c in 0..3 {
                    assert!((a.pos[c] - b.pos[c]).abs() < 1e-4, "cuda sm86 f32 pos mismatch at {i}[{c}]");
                    assert!((a.norm[c] - b.norm[c]).abs() < 1e-4, "cuda sm86 f32 norm mismatch at {i}[{c}]");
                }
            }
        }

        let cu_f64_gen = cuda.eval_bicubic_f64(&patch_f64, &queries_f64, false).expect("cuda generic f64");
        for (i, (a, b)) in ref_f64.iter().zip(&cu_f64_gen).enumerate() {
            for c in 0..3 {
                assert!((a.pos[c] - b.pos[c]).abs() < 1e-12, "cuda sm70 f64 pos mismatch at {i}[{c}]: {} vs {}", a.pos[c], b.pos[c]);
                assert!((a.norm[c] - b.norm[c]).abs() < 1e-12, "cuda sm70 f64 norm mismatch at {i}[{c}]: {} vs {}", a.norm[c], b.norm[c]);
            }
        }

        if cuda.has_fp64_tensor_core() {
            let cu_f64_dmma = cuda.eval_bicubic_f64(&patch_f64, &queries_f64, true).expect("cuda sm86 DMMA tensor core f64");
            for (i, (a, b)) in ref_f64.iter().zip(&cu_f64_dmma).enumerate() {
                for c in 0..3 {
                    assert!(
                        (a.pos[c] - b.pos[c]).abs() < 1e-12,
                        "Ampere DMMA f64 pos mismatch at query {i} comp {c}: cpu={} dmma={}",
                        a.pos[c],
                        b.pos[c]
                    );
                    assert!(
                        (a.norm[c] - b.norm[c]).abs() < 1e-12,
                        "Ampere DMMA f64 norm mismatch at query {i} comp {c}: cpu={} dmma={}",
                        a.norm[c],
                        b.norm[c]
                    );
                }
            }
        }
    }
}

/// Verifies 3-ray watertight point-in-solid classification parity across CPU, Vulkan, CUDA PTX, and RT/BVH backends, including empty-BVH fallback.
#[test]
fn multi_backend_point_in_solid_and_rt_bvh_parity() {
    use crate::cuda::CudaBackend;
    use crate::dispatch::{
        VulkanAccel, build_gpu_bvh_from_mesh, cpu_classify_points_rayon, cpu_classify_points_scalar, synthetic_closed_sphere_mesh,
    };

    let mesh = synthetic_closed_sphere_mesh(10.0, 10, 10);
    let (tris, bvh) = build_gpu_bvh_from_mesh(&mesh);

    let mut pts = Vec::new();
    for ix in -6..=6 {
        for iy in -6..=6 {
            for iz in -4..=4 {
                let p = [ix as f32 * 2.1 + 0.13, iy as f32 * 2.1 - 0.17, iz as f32 * 2.9 + 0.07, 0.0];
                pts.push(p);
            }
        }
    }

    let ref_scalar = cpu_classify_points_scalar(&pts, &tris, &[]);
    let ref_bvh = cpu_classify_points_scalar(&pts, &tris, &bvh);
    let ref_rayon = cpu_classify_points_rayon(&pts, &tris, &bvh);
    assert_eq!(ref_scalar, ref_bvh, "CPU brute-force vs CPU BVH point classification");
    assert_eq!(ref_scalar, ref_rayon, "CPU scalar vs CPU rayon point classification");

    // Verify agreement with SolidCraft's authoritative Mesh::contains
    for (i, p) in pts.iter().enumerate() {
        let v = Vec3::new(p[0] as f64, p[1] as f64, p[2] as f64);
        assert_eq!(ref_scalar[i], mesh.contains(v), "mismatch with Mesh::contains at {i}: {v:?}");
    }

    if let Some(vk) = VulkanAccel::global() {
        let vk_tiled = vk.classify_points(&pts, &tris, &bvh, false).expect("vk classify tiled");
        let vk_bvh = vk.classify_points(&pts, &tris, &bvh, true).expect("vk classify bvh");
        assert_eq!(ref_scalar, vk_tiled, "Vulkan tiled classify_points parity");
        assert_eq!(ref_scalar, vk_bvh, "Vulkan BVH classify_points parity");
        // Empty-BVH with non-empty tris must return None on BVH path so callers fall back to brute-force
        assert!(vk.classify_points(&pts, &tris, &[], true).is_none());
        assert_eq!(vk.classify_points(&pts, &[], &[], true), Some(vec![false; pts.len()]));
    }

    if let Some(cuda) = CudaBackend::global() {
        let cu_gen = cuda.classify_points(&pts, &tris, false).expect("cuda sm70 classify_points");
        assert_eq!(ref_scalar, cu_gen, "CUDA sm_70 classify_points parity");
        if cuda.has_ampere_ptx() {
            let cu_amp = cuda.classify_points(&pts, &tris, true).expect("cuda sm86 classify_points");
            assert_eq!(ref_scalar, cu_amp, "CUDA sm_86 cp.async classify_points parity");
        }
        let cu_rt_bvh = cuda.classify_points_rt_bvh(&pts, &tris, &bvh).expect("cuda rt_bvh classify_points");
        assert_eq!(ref_scalar, cu_rt_bvh, "CUDA RT/BVH classify_points parity");
        // Empty-BVH with non-empty tris must return None so the dispatcher falls back
        assert!(cuda.classify_points_rt_bvh(&pts, &tris, &[]).is_none());
        assert_eq!(cuda.classify_points_rt_bvh(&pts, &[], &[]), Some(vec![false; pts.len()]));
    }
}

/// Verifies BVH ray-mesh intersection parity across CPU, Vulkan, and CUDA RT/BVH backends against `Mesh::raycast`, including empty-BVH handling.
#[test]
fn multi_backend_raycast_bvh_parity() {
    use crate::cuda::{CudaBackend, GpuRay};
    use crate::dispatch::{VulkanAccel, build_gpu_bvh_from_mesh, cpu_raycast_bvh_rayon, cpu_raycast_bvh_scalar, synthetic_closed_sphere_mesh};

    let mesh = synthetic_closed_sphere_mesh(8.0, 10, 10);
    let (tris, bvh) = build_gpu_bvh_from_mesh(&mesh);

    let mut rays = Vec::new();
    for ix in -10..=10 {
        for iy in -10..=10 {
            let ox = ix as f32 * 1.15 + 0.03;
            let oy = iy as f32 * 1.15 - 0.04;
            rays.push(GpuRay { origin: [ox, oy, -20.0, 0.0], dir: [0.0, 0.0, 1.0, 0.0] });
        }
    }

    let ref_hits = cpu_raycast_bvh_scalar(&rays, &tris, &bvh);
    let ray_hits = cpu_raycast_bvh_rayon(&rays, &tris, &bvh);
    assert_eq!(ref_hits, ray_hits);

    // Check against Mesh::raycast
    for (i, r) in rays.iter().enumerate() {
        let orig = Vec3::new(r.origin[0] as f64, r.origin[1] as f64, r.origin[2] as f64);
        let dir = Vec3::new(r.dir[0] as f64, r.dir[1] as f64, r.dir[2] as f64);
        let mesh_hit = mesh.raycast(orig, dir);
        let gpu_hit = ref_hits[i];
        match mesh_hit {
            Some((t_cpu, _)) => {
                assert!(gpu_hit.tri_idx != u32::MAX, "ray {i} missed in GPU BVH but hit in Mesh::raycast at t={t_cpu}");
                assert!((gpu_hit.t as f64 - t_cpu).abs() < 1e-4, "ray {i} t mismatch: {} vs {t_cpu}", gpu_hit.t);
            }
            None => {
                assert_eq!(gpu_hit.tri_idx, u32::MAX, "ray {i} hit in GPU BVH but missed in Mesh::raycast");
            }
        }
    }

    if let Some(vk) = VulkanAccel::global() {
        let vk_hits = vk.raycast_bvh(&rays, &tris, &bvh).expect("vk raycast_bvh");
        for (i, (a, b)) in ref_hits.iter().zip(&vk_hits).enumerate() {
            assert_eq!(a.tri_idx, b.tri_idx, "Vulkan raycast tri_idx mismatch at {i}");
            if a.tri_idx != u32::MAX {
                assert!((a.t - b.t).abs() < 1e-4, "Vulkan raycast t mismatch at {i}");
            }
        }
        assert!(vk.raycast_bvh(&rays, &tris, &[]).is_none());
        let empty_hits = vk.raycast_bvh(&rays, &[], &[]).expect("vk raycast empty tris");
        assert!(empty_hits.iter().all(|h| h.tri_idx == u32::MAX));
    }

    if let Some(cuda) = CudaBackend::global() {
        let cu_hits = cuda.raycast_rt_bvh(&rays, &tris, &bvh).expect("cuda raycast_rt_bvh");
        for (i, (a, b)) in ref_hits.iter().zip(&cu_hits).enumerate() {
            assert_eq!(a.tri_idx, b.tri_idx, "CUDA RT/BVH raycast tri_idx mismatch at {i}");
            if a.tri_idx != u32::MAX {
                assert!((a.t - b.t).abs() < 1e-4, "CUDA RT/BVH raycast t mismatch at {i}");
            }
        }
        assert!(cuda.raycast_rt_bvh(&rays, &tris, &[]).is_none());
        let empty_hits = cuda.raycast_rt_bvh(&rays, &[], &[]).expect("cuda raycast empty tris");
        assert!(empty_hits.iter().all(|h| h.tri_idx == u32::MAX));
    }
}

/// Verifies bipartite AABB overlap, self-overlap, Morton LBVH traversal, and surface-surface intersection (SSI) residual evaluation across CPU, Vulkan, and CUDA backends.
#[test]
fn multi_backend_collision_lbvh_morton_and_ssi_residuals_parity() {
    use crate::collision::{build_simple_lbvh, cpu_bipartite_overlap, cpu_self_overlap, face_aabbs_from_plan, synthetic_aabb_grid};
    use crate::cuda::CudaBackend;
    use crate::dispatch::{VulkanAccel, cpu_bipartite_overlap_rayon};

    let boxes_a = synthetic_aabb_grid(150, 2.0, 1.15);
    let boxes_b = synthetic_aabb_grid(130, 2.0, 1.15);
    let tol = 1e-4f32;

    let cpu_res = cpu_bipartite_overlap(&boxes_a, &boxes_b, tol);
    let ray_res = cpu_bipartite_overlap_rayon(&boxes_a, &boxes_b, tol);
    assert!(!cpu_res.pairs.is_empty());
    assert_eq!(cpu_res.pairs, ray_res.pairs);

    if let Some(vk) = VulkanAccel::global() {
        let vk_res = vk.collision().bipartite_overlap(&boxes_a, &boxes_b, tol).expect("vk bipartite");
        assert_eq!(cpu_res.pairs, vk_res.pairs, "Vulkan bipartite AABB overlap parity");

        // Self-overlap parity
        let cpu_self = cpu_self_overlap(&boxes_a, tol);
        let vk_self = vk.collision().self_overlap(&boxes_a, tol).expect("vk self_overlap");
        assert_eq!(cpu_self.pairs, vk_self.pairs, "Vulkan self_overlap parity");

        // Morton code + LBVH traversal parity
        let morton = vk.collision().compute_morton(&boxes_b).expect("vk morton");
        let lbvh = build_simple_lbvh(&boxes_b, &morton);
        let lbvh_res = vk.collision().lbvh_query(&boxes_a, &lbvh, tol).expect("vk lbvh_query");
        assert_eq!(cpu_res.pairs, lbvh_res.pairs, "Vulkan LBVH traversal vs all-pairs parity");

        // Real B-rep face AABBs & SSI surface residuals parity
        let b1 = ok(solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 4.0, 10.0));
        let b2 = ok(solidcraft_kernel::cylinder(Vec3::new(2.0, -5.0, 5.0), Vec3::Y, 3.0, 10.0));
        let p1 = ok(b1.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.05)));
        let p2 = ok(b2.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.05)));
        let fa = face_aabbs_from_plan(&p1);
        let fb = face_aabbs_from_plan(&p2);
        assert_eq!(cpu_bipartite_overlap(&fa, &fb, 1e-3).pairs, vk.collision().bipartite_overlap(&fa, &fb, 1e-3).expect("vk face aabbs").pairs);

        if let (Some(sa), Some(sb)) = (p1.surfaces.first(), p2.surfaces.first()) {
            let vk_seeds =
                vk.collision().surface_residuals(sa, (0.0, 1.0), (0.0, 1.0), sb, (0.0, 1.0), (0.0, 1.0), 16, 1.5).expect("vk surface_residuals");
            for s in &vk_seeds {
                assert!(s.dist_sq <= 1.5 * 1.5 + 1e-4);
            }
        }
    }

    if let Some(cuda) = CudaBackend::global() {
        let cu_gen = cuda.bipartite_aabb_overlap(&boxes_a, &boxes_b, tol, false).expect("cuda sm70 bipartite");
        assert_eq!(cpu_res.pairs, cu_gen.pairs, "CUDA sm_70 bipartite AABB parity");
        if cuda.has_ampere_ptx() {
            let cu_amp = cuda.bipartite_aabb_overlap(&boxes_a, &boxes_b, tol, true).expect("cuda sm86 bipartite");
            assert_eq!(cpu_res.pairs, cu_amp.pairs, "CUDA sm_86 cp.async + redux.sync bipartite AABB parity");
        }
    }
}

/// Verifies batched point-to-mesh squared distance and divergence-theorem mass properties across CPU, Vulkan, and CUDA backends.
#[test]
fn multi_backend_point_mesh_distance_and_mass_properties_parity() {
    use crate::cuda::CudaBackend;
    use crate::dispatch::{
        VulkanAccel, build_gpu_bvh_from_mesh, cpu_mass_properties_rayon, cpu_mass_properties_scalar, cpu_point_mesh_distance_rayon,
        cpu_point_mesh_distance_scalar,
    };

    let bx_body = ok(solidcraft_kernel::box_solid(Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 5.0, 6.0)));
    let bx_mesh = ok(bx_body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.05)));
    let (tris, _) = build_gpu_bvh_from_mesh(&bx_mesh);

    // 1. Point-to-Mesh Distance parity
    let pts: Vec<[f32; 4]> = (0..195)
        .map(|i| {
            let f = i as f32 * 0.13;
            [f - 5.0, (f * 1.7).sin() * 6.0, (f * 0.9).cos() * 8.0, 0.0]
        })
        .collect();
    let ref_dist = cpu_point_mesh_distance_scalar(&pts, &tris);
    let ray_dist = cpu_point_mesh_distance_rayon(&pts, &tris);
    assert_eq!(ref_dist, ray_dist);

    if let Some(vk) = VulkanAccel::global() {
        let vk_dist = vk.point_mesh_distance(&pts, &tris).expect("vk point_mesh_distance");
        for (i, (a, b)) in ref_dist.iter().zip(&vk_dist).enumerate() {
            assert!((a - b).abs() < 1e-3, "vk distance mismatch at {i}: {a} vs {b}");
        }
    }
    if let Some(cuda) = CudaBackend::global() {
        let cu_gen = cuda.point_mesh_distance(&pts, &tris, false).expect("cuda sm70 dist");
        for (i, (a, b)) in ref_dist.iter().zip(&cu_gen).enumerate() {
            assert!((a - b).abs() < 1e-3, "cuda sm70 distance mismatch at {i}: {a} vs {b}");
        }
        if cuda.has_ampere_ptx() {
            let cu_amp = cuda.point_mesh_distance(&pts, &tris, true).expect("cuda sm86 cp.async dist");
            for (i, (a, b)) in ref_dist.iter().zip(&cu_amp).enumerate() {
                assert!((a - b).abs() < 1e-3, "cuda sm86 distance mismatch at {i}: {a} vs {b}");
            }
        }
    }

    // 2. Mass Properties Integration parity against Mesh::measure()
    let exact_measure = bx_mesh.measure();
    let cpu_s = cpu_mass_properties_scalar(&tris);
    let cpu_r = cpu_mass_properties_rayon(&tris);
    assert!((cpu_s.volume - exact_measure.volume).abs() < 1e-4, "volume: {} vs {}", cpu_s.volume, exact_measure.volume);
    assert!((cpu_s.area - exact_measure.area).abs() < 1e-4, "area: {} vs {}", cpu_s.area, exact_measure.area);
    assert!(cpu_s.centroid.dist(exact_measure.centroid) < 1e-4);
    assert!((cpu_r.volume - exact_measure.volume).abs() < 1e-4);

    if let Some(vk) = VulkanAccel::global() {
        let vk_m = vk.mass_properties(&tris).expect("vk mass_properties");
        assert!((vk_m.volume - exact_measure.volume).abs() < 1e-2);
        assert!((vk_m.area - exact_measure.area).abs() < 1e-2);
        assert!(vk_m.centroid.dist(exact_measure.centroid) < 1e-3);
    }
    if let Some(cuda) = CudaBackend::global() {
        let cu_gen = cuda.mass_properties(&tris, false).expect("cuda sm70 mass_properties");
        assert!((cu_gen.volume - exact_measure.volume).abs() < 1e-4);
        assert!((cu_gen.area - exact_measure.area).abs() < 1e-4);
        assert!(cu_gen.centroid.dist(exact_measure.centroid) < 1e-4);
        if cuda.has_ampere_ptx() {
            let cu_amp = cuda.mass_properties(&tris, true).expect("cuda sm86 shfl.sync mass_properties");
            assert!((cu_amp.volume - exact_measure.volume).abs() < 1e-4);
            assert!((cu_amp.area - exact_measure.area).abs() < 1e-4);
            assert!(cu_amp.centroid.dist(exact_measure.centroid) < 1e-4);
        }
    }

    // 3. Translated mesh cancellation invariance (e.g. translated by (1024, -1024, 256))
    let c0 = Vec3::new(1024.0, -1024.0, 256.0);
    let c1 = c0 + Vec3::new(2.0, 3.0, 4.0);
    let trans_bx = ok(solidcraft_kernel::box_solid(c0, c1));
    let trans_mesh = ok(trans_bx.mesh(&solidcraft_kernel::MeshTolerance::chord(0.01)));
    let (trans_tris, _) = build_gpu_bvh_from_mesh(&trans_mesh);
    let trans_measure = trans_mesh.measure();
    let expected_centroid = c0 + Vec3::new(1.0, 1.5, 2.0);
    assert!((trans_measure.volume - 24.0).abs() < 1e-4);
    assert!(trans_measure.centroid.dist(expected_centroid) < 1e-4);

    let trans_cpu = cpu_mass_properties_scalar(&trans_tris);
    assert!((trans_cpu.volume - 24.0).abs() < 1e-4);
    assert!(trans_cpu.centroid.dist(expected_centroid) < 1e-4);

    if let Some(vk) = VulkanAccel::global() {
        let vk_trans = vk.mass_properties(&trans_tris).expect("vk mass_properties translated");
        assert!((vk_trans.volume - 24.0).abs() < 1e-2, "vk translated volume: {}", vk_trans.volume);
        assert!(vk_trans.centroid.dist(expected_centroid) < 1e-2, "vk translated centroid: {:?}", vk_trans.centroid);
    }
    if let Some(cuda) = CudaBackend::global() {
        let cu_trans = cuda.mass_properties(&trans_tris, false).expect("cuda translated mass_properties");
        assert!((cu_trans.volume - 24.0).abs() < 1e-4);
        assert!(cu_trans.centroid.dist(expected_centroid) < 1e-4);
    }
}

/// Verifies [`Dispatcher`] multi-sample rotated calibration, markdown reporting, and forced routing across all backend targets.
#[test]
fn dispatcher_auto_tuning_calibration_and_routing() {
    use crate::cuda::{GpuRay, PatchUvQueryF32};
    use crate::dispatch::{
        BENCH_SIZES, BackendTarget, CALIBRATION_SAMPLES, Dispatcher, KernelOp, build_gpu_bvh_from_mesh, synthetic_bicubic_patch_f32,
        synthetic_bicubic_patch_f64, synthetic_closed_sphere_mesh,
    };

    let disp = Dispatcher::global();
    let report = disp.report();
    assert_eq!(report.kernels.len(), KernelOp::ALL.len());
    for k in &report.kernels {
        assert_eq!(k.buckets.len(), BENCH_SIZES.len());
        for b in &k.buckets {
            assert_eq!(b.sample_orders.len(), CALIBRATION_SAMPLES);
            assert_eq!(b.sample_timings_ns.len(), CALIBRATION_SAMPLES);
            if b.sample_orders[0].len() > 1 {
                assert_ne!(b.sample_orders[0], b.sample_orders[1], "backend execution order must rotate across calibration samples");
            }
        }
    }

    let md = report.to_markdown();
    println!("\n{md}");
    assert!(md.contains("SolidCraft Multi-Backend GPU & Auto-Tuning Dispatcher Report"));
    assert!(md.contains("Bicubic Tensor Patch Eval (f64 DMMA)"));

    // Verify forced routing across every backend target produces consistent results
    let patch_f32 = synthetic_bicubic_patch_f32();
    let patch_f64 = synthetic_bicubic_patch_f64();
    let q_f32 = vec![PatchUvQueryF32 { u: 0.25, v: 0.75, patch_idx: 0, pad: 0 }; 32];
    let q_f64 = vec![[0.25f64, 0.75, 0.0, 0.0]; 32];
    let sphere = synthetic_closed_sphere_mesh(5.0, 6, 6);
    let (tris, bvh) = build_gpu_bvh_from_mesh(&sphere);
    let pts = vec![[0.5f32, 0.5, 0.5, 0.0], [10.0, 10.0, 10.0, 0.0]];
    let rays = vec![GpuRay { origin: [0.0, 0.0, -10.0, 0.0], dir: [0.0, 0.0, 1.0, 0.0] }];

    let (_, ref_f64) = disp.with_forced_backend(Some(BackendTarget::CpuScalar)).eval_bicubic_f64(&patch_f64, &q_f64);
    let (_, ref_cls) = disp.with_forced_backend(Some(BackendTarget::CpuScalar)).classify_points(&pts, &tris, &bvh);

    for target in [
        BackendTarget::CpuScalar,
        BackendTarget::CpuRayon,
        BackendTarget::VulkanCompute,
        BackendTarget::CudaGeneric,
        BackendTarget::AmperePtx,
        BackendTarget::AmpereTensorCore,
        BackendTarget::RtCoreBvh,
    ] {
        let d = disp.with_forced_backend(Some(target));
        let (_, out_f32) = d.eval_bicubic_f32(&patch_f32, &q_f32);
        assert_eq!(out_f32.len(), 32);
        let (_, out_f64) = d.eval_bicubic_f64(&patch_f64, &q_f64);
        assert!((out_f64[0].pos[0] - ref_f64[0].pos[0]).abs() < 1e-12);
        let (_, out_cls) = d.classify_points(&pts, &tris, &bvh);
        assert_eq!(out_cls, ref_cls);
        let (_, out_cls_no_bvh) = d.classify_points(&pts, &tris, &[]);
        assert_eq!(out_cls_no_bvh, ref_cls);
        let (_, out_rc) = d.raycast_mesh(&rays, &tris, &bvh);
        assert!(out_rc[0].tri_idx != u32::MAX);
        let (_, out_rc_no_bvh) = d.raycast_mesh(&rays, &tris, &[]);
        assert_eq!(out_rc_no_bvh[0].tri_idx, out_rc[0].tri_idx);
        let (_, out_dist) = d.point_mesh_distance(&pts, &tris);
        assert_eq!(out_dist.len(), 2);
        let (_, out_mass) = d.mass_properties(&tris);
        assert!(out_mass.volume > 100.0);
    }
}

/// Extracts sorted `(key, value)` pairs from a `[section]` header in a TOML manifest.
fn parse_toml_section_entries(toml_text: &str, section_header: &str) -> Vec<(String, String)> {
    let mut in_section = false;
    let mut entries = Vec::new();
    for raw_line in toml_text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_section = line == section_header;
            continue;
        }
        if in_section && let Some((k, v)) = line.split_once('=') {
            entries.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    entries.sort();
    entries
}

/// Ensures `[lints.clippy]` and `[lints.rust]` in `crates/gpu/Cargo.toml` stay synchronized with `[workspace.lints.*]` in the root `Cargo.toml`.
#[test]
fn gpu_cargo_toml_lints_match_workspace() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let gpu_toml = ok(std::fs::read_to_string(manifest_dir.join("Cargo.toml")));
    let root_toml = ok(std::fs::read_to_string(manifest_dir.join("../../Cargo.toml")));

    let ws_clippy = parse_toml_section_entries(&root_toml, "[workspace.lints.clippy]");
    let gpu_clippy = parse_toml_section_entries(&gpu_toml, "[lints.clippy]");
    assert!(!ws_clippy.is_empty(), "root Cargo.toml must have [workspace.lints.clippy]");
    assert_eq!(gpu_clippy, ws_clippy, "crates/gpu/Cargo.toml [lints.clippy] must stay in sync with root Cargo.toml [workspace.lints.clippy]");

    let ws_rust = parse_toml_section_entries(&root_toml, "[workspace.lints.rust]");
    let gpu_rust = parse_toml_section_entries(&gpu_toml, "[lints.rust]");
    assert_eq!(ws_rust, vec![("unsafe_code".to_string(), "\"forbid\"".to_string())]);
    assert_eq!(
        gpu_rust,
        vec![("unsafe_code".to_string(), "\"deny\"".to_string())],
        "crates/gpu/Cargo.toml [lints.rust] must match [workspace.lints.rust] except for unsafe_code = \"deny\""
    );
}
