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

/// Verifies that CUDA device info correctly restricts hardware FP64 tensor cores to enterprise architectures (sm_80, sm_90)
/// and does not misidentify consumer Ampere/Ada (sm_86, sm_87, sm_89) GPUs as having hardware FP64 tensor cores.
#[test]
fn cuda_fp64_tensor_core_detection_rules() {
    use crate::cuda::{CudaBackend, CudaDeviceInfo};
    if let Some(cuda) = CudaBackend::global() {
        let dev = cuda.device_info();
        let (major, minor) = dev.compute_capability;
        let expected_hw_fp64 = (major == 8 && minor == 0) || (major == 9 && minor == 0) || (major == 10 && (minor == 0 || minor == 7));
        assert_eq!(
            dev.supports_fp64_tensor_core, expected_hw_fp64,
            "device {} (SM {}.{}) FP64 tensor core flag must be {} (hardware FP64 MMA only exists on sm_80, sm_90, sm_100/107)",
            dev.name, major, minor, expected_hw_fp64
        );
        assert_eq!(cuda.has_fp64_tensor_core(), expected_hw_fp64 && cuda.has_ampere_ptx());
    }

    // Verify `select_best_device` filters out pre-sm_70 devices even when they have more SMs,
    // and breaks ties among eligible sm_70+ devices by `(sm_count, compute_capability, total_vram_bytes)`.
    let mk_dev = |ordinal: i32, cc: (i32, i32), sm_count: i32, vram: usize| CudaDeviceInfo {
        ordinal,
        name: format!("GPU-{ordinal}"),
        compute_capability: cc,
        sm_count,
        clock_khz: 1_500_000,
        memory_bus_bits: 256,
        l2_cache_bytes: 4 * 1024 * 1024,
        total_vram_bytes: vram,
        supports_ampere_ptx: cc.0 >= 8,
        supports_fp64_tensor_core: false,
        supports_rt_core: cc >= (7, 5),
    };
    let pascal_only = [mk_dev(0, (6, 1), 28, 11 << 30), mk_dev(1, (0, 0), 64, 16 << 30)];
    assert!(CudaBackend::select_best_device(&pascal_only).is_none());

    let mixed = [mk_dev(0, (6, 1), 28, 11 << 30), mk_dev(1, (8, 6), 20, 8 << 30), mk_dev(2, (7, 5), 20, 16 << 30)];
    assert_eq!(CudaBackend::select_best_device(&mixed).expect("eligible gpu").ordinal, 1);
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
    use crate::cuda::{CudaBackend, GpuTri};
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

    // 3. Translated and refined mesh cancellation invariance (e.g. translated by (1024, -1024, 256), 768 triangles)
    let c0 = Vec3::new(1024.0, -1024.0, 256.0);
    let verts = [
        c0 + Vec3::new(0.0, 0.0, 0.0),
        c0 + Vec3::new(2.0, 0.0, 0.0),
        c0 + Vec3::new(2.0, 3.0, 0.0),
        c0 + Vec3::new(0.0, 3.0, 0.0),
        c0 + Vec3::new(0.0, 0.0, 4.0),
        c0 + Vec3::new(2.0, 0.0, 4.0),
        c0 + Vec3::new(2.0, 3.0, 4.0),
        c0 + Vec3::new(0.0, 3.0, 4.0),
    ];
    let initial_indices =
        [[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
    let mut current_tris: Vec<[Vec3; 3]> = initial_indices.iter().map(|&[i, j, k]| [verts[i], verts[j], verts[k]]).collect();
    for _ in 0..3 {
        let mut next = Vec::with_capacity(current_tris.len() * 4);
        for &[a, b, c] in &current_tris {
            let ab = (a + b) * 0.5;
            let bc = (b + c) * 0.5;
            let ca = (c + a) * 0.5;
            next.push([a, ab, ca]);
            next.push([ab, b, bc]);
            next.push([ca, bc, c]);
            next.push([ab, bc, ca]);
        }
        current_tris = next;
    }
    assert_eq!(current_tris.len(), 768);
    let trans_tris: Vec<GpuTri> = current_tris
        .iter()
        .enumerate()
        .map(|(idx, &[v0, v1, v2])| GpuTri {
            v0: [v0.x as f32, v0.y as f32, v0.z as f32, f32::from_bits(idx as u32)],
            v1: [v1.x as f32, v1.y as f32, v1.z as f32, 0.0],
            v2: [v2.x as f32, v2.y as f32, v2.z as f32, 0.0],
        })
        .collect();

    let expected_centroid = c0 + Vec3::new(1.0, 1.5, 2.0);
    let trans_cpu = cpu_mass_properties_scalar(&trans_tris);
    assert!((trans_cpu.volume - 24.0).abs() < 1e-4);
    assert!((trans_cpu.area - 52.0).abs() < 1e-4);
    assert!(trans_cpu.centroid.dist(expected_centroid) < 1e-4);

    if let Some(vk) = VulkanAccel::global() {
        let vk_trans = vk.mass_properties(&trans_tris).expect("vk mass_properties translated 768 tris");
        assert!((vk_trans.volume - 24.0).abs() < 1e-2, "vk translated volume: {}", vk_trans.volume);
        assert!((vk_trans.area - 52.0).abs() < 1e-2, "vk translated area: {}", vk_trans.area);
        assert!(vk_trans.centroid.dist(expected_centroid) < 1e-2, "vk translated centroid: {:?}", vk_trans.centroid);
    }
    if let Some(cuda) = CudaBackend::global() {
        let cu_trans = cuda.mass_properties(&trans_tris, false).expect("cuda translated mass_properties 768 tris");
        assert!((cu_trans.volume - 24.0).abs() < 1e-4);
        assert!((cu_trans.area - 52.0).abs() < 1e-4);
        assert!(cu_trans.centroid.dist(expected_centroid) < 1e-4);
    }

    // 4. Two separated closed components (Box A at origin and Box B at (1024, -1024, 256), 768 triangles each)
    let make_subdivided_box = |offset: Vec3| -> Vec<[Vec3; 3]> {
        let v = [
            offset + Vec3::new(0.0, 0.0, 0.0),
            offset + Vec3::new(2.0, 0.0, 0.0),
            offset + Vec3::new(2.0, 3.0, 0.0),
            offset + Vec3::new(0.0, 3.0, 0.0),
            offset + Vec3::new(0.0, 0.0, 4.0),
            offset + Vec3::new(2.0, 0.0, 4.0),
            offset + Vec3::new(2.0, 3.0, 4.0),
            offset + Vec3::new(0.0, 3.0, 4.0),
        ];
        let mut cur: Vec<[Vec3; 3]> = initial_indices.iter().map(|&[i, j, k]| [v[i], v[j], v[k]]).collect();
        for _ in 0..3 {
            let mut nxt = Vec::with_capacity(cur.len() * 4);
            for &[a, b, c] in &cur {
                let ab = (a + b) * 0.5;
                let bc = (b + c) * 0.5;
                let ca = (c + a) * 0.5;
                nxt.push([a, ab, ca]);
                nxt.push([ab, b, bc]);
                nxt.push([ca, bc, c]);
                nxt.push([ab, bc, ca]);
            }
            cur = nxt;
        }
        cur
    };

    let box_a = make_subdivided_box(Vec3::ZERO);
    let box_b = make_subdivided_box(c0);
    let expected_two_box_centroid = Vec3::new(513.0, -510.5, 130.0);

    for (desc, tris_list) in [("A then B", [&box_a[..], &box_b[..]].concat()), ("B then A", [&box_b[..], &box_a[..]].concat())] {
        let two_box_gpu: Vec<GpuTri> = tris_list
            .iter()
            .enumerate()
            .map(|(idx, &[v0, v1, v2])| GpuTri {
                v0: [v0.x as f32, v0.y as f32, v0.z as f32, f32::from_bits(idx as u32)],
                v1: [v1.x as f32, v1.y as f32, v1.z as f32, 0.0],
                v2: [v2.x as f32, v2.y as f32, v2.z as f32, 0.0],
            })
            .collect();
        assert_eq!(two_box_gpu.len(), 1536);

        let cpu_m = cpu_mass_properties_scalar(&two_box_gpu);
        assert!((cpu_m.volume - 48.0).abs() < 1e-4, "{desc} cpu volume: {}", cpu_m.volume);
        assert!((cpu_m.area - 104.0).abs() < 1e-4, "{desc} cpu area: {}", cpu_m.area);
        assert!(cpu_m.centroid.dist(expected_two_box_centroid) < 1e-4, "{desc} cpu centroid: {:?}", cpu_m.centroid);

        if let Some(vk) = VulkanAccel::global() {
            let vk_m = vk.mass_properties(&two_box_gpu).expect("vk mass_properties two-box");
            assert!((vk_m.volume - 48.0).abs() < 1e-2, "{desc} vk volume: {}", vk_m.volume);
            assert!((vk_m.area - 104.0).abs() < 1e-2, "{desc} vk area: {}", vk_m.area);
            assert!(vk_m.centroid.dist(expected_two_box_centroid) < 1e-2, "{desc} vk centroid: {:?}", vk_m.centroid);
        }
        if let Some(cuda) = CudaBackend::global() {
            let cu_m = cuda.mass_properties(&two_box_gpu, false).expect("cuda mass_properties two-box");
            assert!((cu_m.volume - 48.0).abs() < 1e-4, "{desc} cuda volume: {}", cu_m.volume);
            assert!((cu_m.area - 104.0).abs() < 1e-4, "{desc} cuda area: {}", cu_m.area);
            assert!(cu_m.centroid.dist(expected_two_box_centroid) < 1e-4, "{desc} cuda centroid: {:?}", cu_m.centroid);
        }
    }
}

/// All 7 backend execution targets in [`BackendTarget`].
#[cfg(not(target_arch = "wasm32"))]
const ALL_BACKEND_TARGETS: [crate::dispatch::BackendTarget; 7] = [
    crate::dispatch::BackendTarget::CpuScalar,
    crate::dispatch::BackendTarget::CpuRayon,
    crate::dispatch::BackendTarget::VulkanCompute,
    crate::dispatch::BackendTarget::CudaGeneric,
    crate::dispatch::BackendTarget::AmperePtx,
    crate::dispatch::BackendTarget::AmpereTensorCore,
    crate::dispatch::BackendTarget::RtCoreBvh,
];

/// Backend targets that implement bicubic patch evaluation (`f32` and `f64`).
#[cfg(not(target_arch = "wasm32"))]
const PATCH_TARGETS: [crate::dispatch::BackendTarget; 6] = [
    crate::dispatch::BackendTarget::CpuScalar,
    crate::dispatch::BackendTarget::CpuRayon,
    crate::dispatch::BackendTarget::VulkanCompute,
    crate::dispatch::BackendTarget::CudaGeneric,
    crate::dispatch::BackendTarget::AmperePtx,
    crate::dispatch::BackendTarget::AmpereTensorCore,
];

/// Backend targets that implement BVH queries (point-in-solid, raycast, AABB overlap).
#[cfg(not(target_arch = "wasm32"))]
const BVH_TARGETS: [crate::dispatch::BackendTarget; 6] = [
    crate::dispatch::BackendTarget::CpuScalar,
    crate::dispatch::BackendTarget::CpuRayon,
    crate::dispatch::BackendTarget::VulkanCompute,
    crate::dispatch::BackendTarget::CudaGeneric,
    crate::dispatch::BackendTarget::AmperePtx,
    crate::dispatch::BackendTarget::RtCoreBvh,
];

/// Backend targets that implement general compute kernels (point-mesh distance, mass properties, non-BVH queries).
#[cfg(not(target_arch = "wasm32"))]
const COMPUTE_TARGETS: [crate::dispatch::BackendTarget; 5] = [
    crate::dispatch::BackendTarget::CpuScalar,
    crate::dispatch::BackendTarget::CpuRayon,
    crate::dispatch::BackendTarget::VulkanCompute,
    crate::dispatch::BackendTarget::CudaGeneric,
    crate::dispatch::BackendTarget::AmperePtx,
];

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

    for target in ALL_BACKEND_TARGETS {
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

#[test]
fn bicubic_patch_common_and_edge_cases() {
    use crate::cuda::{CudaBackend, PatchUvQueryF32};
    use crate::dispatch::{
        Dispatcher, cpu_eval_bicubic_f32_rayon, cpu_eval_bicubic_f32_scalar, cpu_eval_bicubic_f64_rayon, cpu_eval_bicubic_f64_scalar,
        synthetic_bicubic_patch_f32, synthetic_bicubic_patch_f64,
    };

    let disp = Dispatcher::global();
    let cuda = CudaBackend::global();

    // 1. Exact corner interpolation on synthetic rational bicubic patch:
    // At (u, v) in {(0,0), (1,0), (0,1), (1,1)}, S(u, v) = P_{3u, 3v} / w_{3u, 3v}.
    let cpts_f64 = synthetic_bicubic_patch_f64();
    let cpts_f32 = synthetic_bicubic_patch_f32();
    let corners = [(0.0, 0.0, 0usize, 0usize), (1.0, 0.0, 3, 0), (0.0, 1.0, 0, 3), (1.0, 1.0, 3, 3)];
    let corner_queries_f64: Vec<[f64; 4]> = corners.iter().map(|&(u, v, _, _)| [u, v, 0.0, 0.0]).collect();
    let corner_queries_f32: Vec<PatchUvQueryF32> =
        corners.iter().map(|&(u, v, _, _)| PatchUvQueryF32 { u: u as f32, v: v as f32, patch_idx: 0, pad: 0 }).collect();

    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, evals_f64) = d.eval_bicubic_f64(&cpts_f64, &corner_queries_f64);
        let (_, evals_f32) = d.eval_bicubic_f32(&cpts_f32, &corner_queries_f32);
        for (idx, &(_, _, i, j)) in corners.iter().enumerate() {
            let cpt = cpts_f64[i * 4 + j];
            let expected = [cpt[0] / cpt[3], cpt[1] / cpt[3], cpt[2] / cpt[3]];
            for (k, &exp) in expected.iter().enumerate() {
                assert!(
                    (evals_f64[idx].pos[k] - exp).abs() < 1e-12,
                    "f64 corner {idx} axis {k} mismatch on {target:?}: got {}, expected {exp}",
                    evals_f64[idx].pos[k],
                );
                assert!(
                    (f64::from(evals_f32[idx].pos[k]) - exp).abs() < 1e-5,
                    "f32 corner {idx} axis {k} mismatch on {target:?}: got {}, expected {exp}",
                    evals_f32[idx].pos[k],
                );
            }
            let n_len_f64 = (evals_f64[idx].norm[0].powi(2) + evals_f64[idx].norm[1].powi(2) + evals_f64[idx].norm[2].powi(2)).sqrt();
            assert!((n_len_f64 - 1.0).abs() < 1e-12, "unit normal required at corner {idx} on {target:?}");
            assert!(evals_f64[idx].norm[3] > 0.0, "positive |Su x Sv| required at corner {idx} on {target:?}");
        }
    }

    // 2. Analytical planar patch S(u, v) = (2u + 1, 3v - 2, 5):
    // Control points at (i/3, j/3) with weight 1 -> dS/du = (2, 0, 0), dS/dv = (0, 3, 0),
    // Su x Sv = (0, 0, 6) -> unit normal = (0, 0, 1), |Su x Sv| = 6.0 everywhere.
    let mut planar_f64 = Vec::with_capacity(16);
    let mut planar_f32 = Vec::with_capacity(16);
    for i in 0..4 {
        for j in 0..4 {
            let u = f64::from(i) / 3.0;
            let v = f64::from(j) / 3.0;
            let x = 2.0 * u + 1.0;
            let y = 3.0 * v - 2.0;
            let z = 5.0;
            planar_f64.push([x, y, z, 1.0]);
            planar_f32.push([x as f32, y as f32, z as f32, 1.0]);
        }
    }
    let sample_uvs = [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.3, 0.7), (0.5, 0.5), (1.0, 1.0)];
    let planar_q64: Vec<[f64; 4]> = sample_uvs.iter().map(|&(u, v)| [u, v, 0.0, 0.0]).collect();
    let planar_q32: Vec<PatchUvQueryF32> =
        sample_uvs.iter().map(|&(u, v)| PatchUvQueryF32 { u: u as f32, v: v as f32, patch_idx: 0, pad: 0 }).collect();

    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, out64) = d.eval_bicubic_f64(&planar_f64, &planar_q64);
        let (_, out32) = d.eval_bicubic_f32(&planar_f32, &planar_q32);
        for (idx, &(u, v)) in sample_uvs.iter().enumerate() {
            let exp_x = 2.0 * u + 1.0;
            let exp_y = 3.0 * v - 2.0;
            assert!((out64[idx].pos[0] - exp_x).abs() < 1e-12);
            assert!((out64[idx].pos[1] - exp_y).abs() < 1e-12);
            assert!((out64[idx].pos[2] - 5.0).abs() < 1e-12);
            assert!(out64[idx].norm[0].abs() < 1e-12);
            assert!(out64[idx].norm[1].abs() < 1e-12);
            assert!((out64[idx].norm[2] - 1.0).abs() < 1e-12);
            assert!((out64[idx].norm[3] - 6.0).abs() < 1e-11);

            assert!((f64::from(out32[idx].pos[0]) - exp_x).abs() < 1e-5);
            assert!((f64::from(out32[idx].pos[1]) - exp_y).abs() < 1e-5);
            assert!((f64::from(out32[idx].pos[2]) - 5.0).abs() < 1e-5);
            assert!((f64::from(out32[idx].norm[2]) - 1.0).abs() < 1e-5);
            assert!((f64::from(out32[idx].norm[3]) - 6.0).abs() < 1e-4);
        }
    }

    // 3. Multi-patch batching (patch_idx = 0, 1, 2) and out-of-bounds patch_idx clamping (patch_idx = 999 -> 2):
    let mut multi_f64 = Vec::with_capacity(48);
    let mut multi_f32 = Vec::with_capacity(48);
    for p_idx in 0..3 {
        let z_offset = f64::from(p_idx) * 10.0;
        for cpt in &cpts_f64 {
            let w = cpt[3];
            let shifted = [cpt[0], cpt[1], cpt[2] + z_offset * w, w];
            multi_f64.push(shifted);
            multi_f32.push([shifted[0] as f32, shifted[1] as f32, shifted[2] as f32, shifted[3] as f32]);
        }
    }
    let multi_q64 = vec![[0.4, 0.6, 0.0, 0.0], [0.4, 0.6, 1.0, 0.0], [0.4, 0.6, 2.0, 0.0], [0.4, 0.6, 999.0, 0.0]];
    let multi_q32 = vec![
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 0, pad: 0 },
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 1, pad: 0 },
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 2, pad: 0 },
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 999, pad: 0 },
    ];
    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, m64) = d.eval_bicubic_f64(&multi_f64, &multi_q64);
        let (_, m32) = d.eval_bicubic_f32(&multi_f32, &multi_q32);
        assert!((m64[1].pos[2] - (m64[0].pos[2] + 10.0)).abs() < 1e-11, "patch 1 z shift failed on {target:?}");
        assert!((m64[2].pos[2] - (m64[0].pos[2] + 20.0)).abs() < 1e-11, "patch 2 z shift failed on {target:?}");
        assert!((m64[3].pos[2] - m64[2].pos[2]).abs() < 1e-12, "out-of-bounds patch_idx must clamp to last patch on {target:?}");
        assert!((m32[1].pos[2] - (m32[0].pos[2] + 10.0)).abs() < 1e-4, "f32 patch 1 z shift failed on {target:?}");
        assert!((m32[2].pos[2] - (m32[0].pos[2] + 20.0)).abs() < 1e-4, "f32 patch 2 z shift failed on {target:?}");
        assert!((m32[3].pos[2] - m32[2].pos[2]).abs() < 1e-5, "f32 out-of-bounds patch_idx must clamp on {target:?}");
    }

    // 4. Degenerate singular patch (all 16 control points collapsed to a single point (3, -4, 7, 1)):
    // Derivatives Su and Sv are zero, so Su x Sv = 0. Must not produce NaN; fallback normal is [0, 0, 1, 0].
    let singular_f64 = vec![[3.0f64, -4.0, 7.0, 1.0]; 16];
    let singular_f32 = vec![[3.0f32, -4.0, 7.0, 1.0]; 16];
    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, s64) = d.eval_bicubic_f64(&singular_f64, &[[0.5, 0.5, 0.0, 0.0]]);
        let (_, s32) = d.eval_bicubic_f32(&singular_f32, &[PatchUvQueryF32 { u: 0.5, v: 0.5, patch_idx: 0, pad: 0 }]);
        assert!((s64[0].pos[0] - 3.0).abs() < 1e-12 && (s64[0].pos[1] + 4.0).abs() < 1e-12 && (s64[0].pos[2] - 7.0).abs() < 1e-12);
        assert_eq!(s64[0].norm, [0.0, 0.0, 1.0, 0.0], "singular patch fallback normal mismatch on {target:?}");
        assert!((s32[0].pos[0] - 3.0).abs() < 1e-5 && (s32[0].pos[1] + 4.0).abs() < 1e-5 && (s32[0].pos[2] - 7.0).abs() < 1e-5);
        assert_eq!(s32[0].norm, [0.0, 0.0, 1.0, 0.0], "f32 singular patch fallback normal mismatch on {target:?}");
    }

    // 5. Empty and insufficient control points + warp/tile boundary sizes (1, 3, 4, 15, 16, 17, 31, 32, 33, 63, 64, 65, 129):
    assert!(cpu_eval_bicubic_f32_scalar(&cpts_f32[..15], &corner_queries_f32).is_empty());
    assert!(cpu_eval_bicubic_f32_rayon(&cpts_f32[..15], &corner_queries_f32).is_empty());
    assert!(cpu_eval_bicubic_f64_scalar(&cpts_f64[..15], &corner_queries_f64).is_empty());
    assert!(cpu_eval_bicubic_f64_rayon(&cpts_f64[..15], &corner_queries_f64).is_empty());
    assert!(cpu_eval_bicubic_f32_scalar(&cpts_f32, &[]).is_empty());
    assert!(cpu_eval_bicubic_f64_scalar(&cpts_f64, &[]).is_empty());

    for n in [1usize, 3, 4, 15, 16, 17, 31, 32, 33, 63, 64, 65, 129] {
        let q32: Vec<PatchUvQueryF32> = (0..n)
            .map(|i| PatchUvQueryF32 { u: (i as f32 + 0.5) / (n as f32 + 1.0), v: ((n - i) as f32 + 0.25) / (n as f32 + 1.0), patch_idx: 0, pad: 0 })
            .collect();
        let q64: Vec<[f64; 4]> = q32.iter().map(|q| [f64::from(q.u), f64::from(q.v), 0.0, 0.0]).collect();
        let ref32 = cpu_eval_bicubic_f32_scalar(&cpts_f32, &q32);
        let ref64 = cpu_eval_bicubic_f64_scalar(&cpts_f64, &q64);
        assert_eq!(ref32.len(), n);
        assert_eq!(ref64.len(), n);
        if let Some(dev) = cuda {
            let gen64 = dev.eval_bicubic_f64(&cpts_f64, &q64, false).expect("cuda gen64");
            for i in 0..n {
                for k in 0..3 {
                    assert!((gen64[i].pos[k] - ref64[i].pos[k]).abs() < 1e-12, "gen64 boundary n={n} i={i} mismatch");
                }
            }
            if dev.has_ampere_ptx() {
                let sm86_32 = dev.eval_bicubic_f32(&cpts_f32, &q32, true).expect("cuda sm86_32");
                for i in 0..n {
                    for k in 0..3 {
                        assert!((sm86_32[i].pos[k] - ref32[i].pos[k]).abs() < 1e-5, "sm86_32 boundary n={n} i={i} mismatch");
                    }
                }
            }
            if dev.has_fp64_tensor_core() {
                let dmma = dev.eval_bicubic_f64(&cpts_f64, &q64, true).expect("cuda dmma");
                for i in 0..n {
                    for k in 0..3 {
                        assert!((dmma[i].pos[k] - ref64[i].pos[k]).abs() < 1e-12, "DMMA boundary n={n} i={i} mismatch");
                    }
                }
            }
        }
    }
}

#[test]
fn point_in_solid_common_and_edge_cases() {
    use crate::cuda::GpuTri;
    use crate::dispatch::{Dispatcher, build_gpu_bvh_from_mesh, cpu_classify_points_scalar};

    let disp = Dispatcher::global();

    // 1. Non-convex Genus-1 Torus (major_r = 10.0, minor_r = 3.0, centered at origin around Z):
    // Central hole (r < 7.0) is outside; tube interior ((sqrt(x^2+y^2) - 10)^2 + z^2 < 9) is inside.
    let torus_solid = ok(solidcraft_kernel::torus(Vec3::ZERO, 10.0, 3.0));
    let torus_mesh = ok(torus_solid.mesh(&solidcraft_kernel::MeshTolerance::chord(0.15)));
    let (torus_tris, torus_bvh) = build_gpu_bvh_from_mesh(&torus_mesh);

    let torus_cases: &[([f32; 4], bool, &str)] = &[
        ([0.0, 0.0, 0.0, 0.0], false, "origin in central torus hole"),
        ([4.0, 0.0, 0.0, 0.0], false, "off-center in central torus hole"),
        ([10.0, 0.0, 0.0, 0.0], true, "center of torus tube +X"),
        ([-10.0, 0.0, 1.0, 0.0], true, "inside torus tube -X/+Z"),
        ([0.0, 10.0, -1.5, 0.0], true, "inside torus tube +Y/-Z"),
        ([7.071, 7.071, 0.0, 0.0], true, "inside torus tube 45 deg"),
        ([10.0, 0.0, 4.5, 0.0], false, "above torus tube"),
        ([15.0, 0.0, 0.0, 0.0], false, "outside outer torus radius"),
    ];
    let torus_pts: Vec<[f32; 4]> = torus_cases.iter().map(|&(p, _, _)| p).collect();

    for target in BVH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, cls_bvh) = d.classify_points(&torus_pts, &torus_tris, &torus_bvh);
        let (_, cls_raw) = d.classify_points(&torus_pts, &torus_tris, &[]);
        for (i, &(_, exp, label)) in torus_cases.iter().enumerate() {
            assert_eq!(cls_bvh[i], exp, "torus BVH classify mismatch for '{label}' on {target:?}");
            assert_eq!(cls_raw[i], exp, "torus raw classify mismatch for '{label}' on {target:?}");
        }
    }

    // 2. Boolean Cut: 10x10x10 box with a vertical through-hole cylinder of radius 2.5 at (5, 5, z):
    let outer_box = ok(solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)));
    let hole_cyl = ok(solidcraft_kernel::cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.5, 12.0));
    let hollow_box = ok(solidcraft_kernel::boolean(&outer_box, &hole_cyl, BoolOp::Cut)).expect("boolean cut");
    let hollow_mesh = ok(hollow_box.mesh(&solidcraft_kernel::MeshTolerance::chord(0.1)));
    let (hollow_tris, hollow_bvh) = build_gpu_bvh_from_mesh(&hollow_mesh);

    let hollow_cases: &[([f32; 4], bool, &str)] = &[
        ([5.0, 5.0, 5.0, 0.0], false, "center of cylindrical through-hole"),
        ([6.0, 5.0, 2.0, 0.0], false, "inside cylindrical through-hole r=1"),
        ([1.5, 1.5, 5.0, 0.0], true, "inside solid corner wall"),
        ([8.5, 5.0, 5.0, 0.0], true, "inside solid wall at r=3.5"),
        ([-1.0, 5.0, 5.0, 0.0], false, "outside box -X"),
        ([5.0, 5.0, 11.0, 0.0], false, "above box +Z"),
    ];
    let hollow_pts: Vec<[f32; 4]> = hollow_cases.iter().map(|&(p, _, _)| p).collect();
    for target in BVH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, cls) = d.classify_points(&hollow_pts, &hollow_tris, &hollow_bvh);
        for (i, &(_, exp, label)) in hollow_cases.iter().enumerate() {
            assert_eq!(cls[i], exp, "hollow box classify mismatch for '{label}' on {target:?}");
        }
    }

    // 3. Degenerate (zero-area collinear/coincident) triangles mixed into a closed mesh + shared-edge ray alignment:
    let mut tris_with_degen = hollow_tris.clone();
    tris_with_degen.push(GpuTri { v0: [5.0, 5.0, 5.0, 0.0], v1: [5.0, 5.0, 5.0, 0.0], v2: [5.0, 5.0, 5.0, 0.0] });
    tris_with_degen.push(GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [1.0, 1.0, 1.0, 0.0], v2: [2.0, 2.0, 2.0, 0.0] });

    // Also include points aligned with box vertices/edges (0,0,0), (5,0,5) and tile-boundary batch sizes (129 points)
    let mut edge_pts = hollow_pts.clone();
    while edge_pts.len() < 129 {
        let idx = edge_pts.len();
        let t = idx as f32 / 129.0;
        edge_pts.push([1.0 + t * 0.5, 1.0 + t * 0.5, 1.0 + t * 8.0, 0.0]);
    }
    let ref_edge = cpu_classify_points_scalar(&edge_pts, &tris_with_degen, &[]);
    for target in COMPUTE_TARGETS {
        let (_, got) = disp.with_forced_backend(Some(target)).classify_points(&edge_pts, &tris_with_degen, &[]);
        assert_eq!(got, ref_edge, "degenerate-tri + 129-point classify parity failed on {target:?}");
    }

    // 4. Empty points / empty triangles edge cases:
    for target in BVH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        assert!(d.classify_points(&[], &hollow_tris, &hollow_bvh).1.is_empty());
        assert_eq!(d.classify_points(&hollow_pts, &[], &[]).1, vec![false; hollow_pts.len()]);
    }
}

#[test]
fn raycast_bvh_common_and_edge_cases() {
    use crate::cuda::{GpuRay, GpuTri};
    use crate::dispatch::{BackendTarget, Dispatcher, build_gpu_bvh_from_mesh};

    let disp = Dispatcher::global();

    // 1. Torus multi-surface piercing ray:
    // A ray from (-20, 0, 0) along +X pierces the torus at x = -13 (entry 1), -7 (exit 1), +7 (entry 2), +13 (exit 2).
    // Closest hit must be the first entry wall near t = 7.0 (x = -13.0).
    let torus_solid = ok(solidcraft_kernel::torus(Vec3::ZERO, 10.0, 3.0));
    let torus_mesh = ok(torus_solid.mesh(&solidcraft_kernel::MeshTolerance::chord(0.1)));
    let (torus_tris, torus_bvh) = build_gpu_bvh_from_mesh(&torus_mesh);

    // 2. Axis-aligned rays (+X, -X, +Y, -Y, +Z, -Z where two direction components are 0.0 -> inv_d = +/-inf),
    // diagonal rays, rays starting inside a solid, and rays pointing away from the solid.
    // Slightly offset the first 3 rays off the exact (y=0, z=0) tessellation vertex seam so each ray hits a unique triangle interior.
    let rays = vec![
        // Pierces 4 torus walls along +X; closest is at x ~= -13 -> t ~= 7.0
        GpuRay { origin: [-20.0, 0.13, 0.07, 0.0], dir: [1.0, 0.0, 0.0, 0.0] },
        // Pierces 4 torus walls along -X; closest is at x ~= +13 -> t ~= 7.0
        GpuRay { origin: [20.0, 0.13, 0.07, 0.0], dir: [-1.0, 0.0, 0.0, 0.0] },
        // Pierces 4 torus walls along +Y; closest is at y ~= -13 -> t ~= 7.0
        GpuRay { origin: [0.13, -20.0, 0.07, 0.0], dir: [0.0, 1.0, 0.0, 0.0] },
        // Passes straight through the central hole of the torus along +Z -> MISS
        GpuRay { origin: [0.0, 0.0, -20.0, 0.0], dir: [0.0, 0.0, 1.0, 0.0] },
        // Hits top of torus tube at (10, 0, +3) from z = +10 along -Z -> t ~= 7.0
        GpuRay { origin: [10.0, 0.13, 10.0, 0.0], dir: [0.0, 0.0, -1.0, 0.0] },
        // Origin inside torus tube (10, 0, 0) shooting +Z -> hits top wall at z = +3 -> t ~= 3.0
        GpuRay { origin: [10.0, 0.13, 0.0, 0.0], dir: [0.0, 0.0, 1.0, 0.0] },
        // Origin outside shooting AWAY from torus -> MISS
        GpuRay { origin: [20.0, 0.0, 0.0, 0.0], dir: [1.0, 0.0, 0.0, 0.0] },
    ];

    let (_, ref_hits) = disp.with_forced_backend(Some(BackendTarget::CpuScalar)).raycast_mesh(&rays, &torus_tris, &torus_bvh);
    assert!((ref_hits[0].t - 7.0).abs() < 0.15 && ref_hits[0].tri_idx != u32::MAX);
    assert!((ref_hits[1].t - 7.0).abs() < 0.15 && ref_hits[1].tri_idx != u32::MAX);
    assert!((ref_hits[2].t - 7.0).abs() < 0.15 && ref_hits[2].tri_idx != u32::MAX);
    assert_eq!(ref_hits[3].tri_idx, u32::MAX, "ray through torus hole must miss");
    assert!((ref_hits[4].t - 7.0).abs() < 0.15 && ref_hits[4].tri_idx != u32::MAX);
    assert!((ref_hits[5].t - 3.0).abs() < 0.15 && ref_hits[5].tri_idx != u32::MAX);
    assert_eq!(ref_hits[6].tri_idx, u32::MAX, "ray pointing away from torus must miss");

    for target in BVH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, hits_bvh) = d.raycast_mesh(&rays, &torus_tris, &torus_bvh);
        let (_, hits_raw) = d.raycast_mesh(&rays, &torus_tris, &[]);
        for (i, r) in ref_hits.iter().enumerate() {
            assert_eq!(hits_bvh[i].tri_idx, r.tri_idx, "torus BVH ray {i} tri_idx mismatch on {target:?}");
            assert!((hits_bvh[i].t - r.t).abs() < 1e-4, "torus BVH ray {i} t mismatch on {target:?}");
            assert_eq!(hits_raw[i].tri_idx, r.tri_idx, "torus no-BVH ray {i} tri_idx mismatch on {target:?}");
            assert!((hits_raw[i].t - r.t).abs() < 1e-4, "torus no-BVH ray {i} t mismatch on {target:?}");
        }
    }

    // 3. Single-triangle leaf BVH + near-origin self-intersection guard (t <= 1e-7 ignored):
    let single_tri = vec![GpuTri { v0: [-1.0, -1.0, 0.0, f32::from_bits(0)], v1: [2.0, -1.0, 0.0, 0.0], v2: [-1.0, 2.0, 0.0, 0.0] }];
    let single_bvh = vec![crate::cuda::GpuBvhNode {
        min_x: -1.0,
        min_y: -1.0,
        min_z: 0.0,
        left_or_prim: 0,
        max_x: 2.0,
        max_y: 2.0,
        max_z: 0.0,
        right_or_count: 0x8000_0000 | 1,
    }];
    let single_rays = vec![
        // Hits at t = 5.0
        GpuRay { origin: [0.0, 0.0, -5.0, 0.0], dir: [0.0, 0.0, 1.0, 0.0] },
        // Starts directly on the triangle (t = 0.0 <= 1e-7) -> must be ignored as self-intersection
        GpuRay { origin: [0.0, 0.0, 0.0, 0.0], dir: [0.0, 0.0, 1.0, 0.0] },
        // Parallel coplanar ray -> must miss
        GpuRay { origin: [-5.0, 0.0, 0.0, 0.0], dir: [1.0, 0.0, 0.0, 0.0] },
    ];
    for target in BVH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, sh) = d.raycast_mesh(&single_rays, &single_tri, &single_bvh);
        assert_eq!(sh[0].tri_idx, 0, "single-tri hit failed on {target:?}");
        assert!((sh[0].t - 5.0).abs() < 1e-5);
        assert_eq!(sh[1].tri_idx, u32::MAX, "t=0 self-intersection must be ignored on {target:?}");
        assert_eq!(sh[2].tri_idx, u32::MAX, "parallel coplanar ray must miss on {target:?}");
        assert!(d.raycast_mesh(&[], &single_tri, &single_bvh).1.is_empty());
        let (_, empty_mesh_hits) = d.raycast_mesh(&single_rays, &[], &[]);
        assert!(empty_mesh_hits.iter().all(|h| h.tri_idx == u32::MAX && h.t < 0.0));
    }

    // 4. Caller-supplied deep BVH exceeding BVH_STACK_CAP (64) must be rejected by bvh_fits_traversal_stack
    // and routed through the non-BVH fallback across Vulkan, CUDA, and CPU without missing hits or miscounting crossings:
    assert!(crate::dispatch::bvh_fits_traversal_stack(&torus_bvh, torus_tris.len()));
    let mut deep_bvh = Vec::with_capacity(141);
    for d in 0..70u32 {
        deep_bvh.push(crate::cuda::GpuBvhNode {
            min_x: -20.0,
            min_y: -20.0,
            min_z: -20.0,
            left_or_prim: d + 1,
            max_x: 20.0,
            max_y: 20.0,
            max_z: 20.0,
            right_or_count: 71 + d,
        });
    }
    // Node 70 is the deep leaf referencing all torus_tris; nodes 71..141 are empty right-sibling leaves
    deep_bvh.push(crate::cuda::GpuBvhNode {
        min_x: -20.0,
        min_y: -20.0,
        min_z: -20.0,
        left_or_prim: 0,
        max_x: 20.0,
        max_y: 20.0,
        max_z: 20.0,
        right_or_count: 0x8000_0000 | (torus_tris.len() as u32),
    });
    for _ in 0..70 {
        deep_bvh.push(crate::cuda::GpuBvhNode {
            min_x: -20.0,
            min_y: -20.0,
            min_z: -20.0,
            left_or_prim: 0,
            max_x: 20.0,
            max_y: 20.0,
            max_z: 20.0,
            right_or_count: 0x8000_0000,
        });
    }
    assert!(!crate::dispatch::bvh_fits_traversal_stack(&deep_bvh, torus_tris.len()), "70-deep BVH must exceed BVH_STACK_CAP");
    if let Some(vk) = crate::dispatch::VulkanAccel::global() {
        let vk_deep_rc = vk.raycast_bvh(&rays, &torus_tris, &deep_bvh).expect("vk deep BVH raycast fallback");
        for (i, r) in ref_hits.iter().enumerate() {
            assert_eq!(vk_deep_rc[i].tri_idx, r.tri_idx, "vk deep BVH ray {i} fallback mismatch");
        }
        let probe_pts = vec![[0.0f32, 0.0, 0.0, 0.0], [10.0, 0.0, 0.0, 0.0]];
        let vk_deep_cls = vk.classify_points(&probe_pts, &torus_tris, &deep_bvh, true).expect("vk deep BVH classify fallback");
        assert_eq!(vk_deep_cls, vec![false, true]);
    }
    for target in BVH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, deep_hits) = d.raycast_mesh(&rays, &torus_tris, &deep_bvh);
        for (i, r) in ref_hits.iter().enumerate() {
            assert_eq!(deep_hits[i].tri_idx, r.tri_idx, "deep BVH ray {i} mismatch on {target:?}");
        }
        let probe_pts = vec![[0.0f32, 0.0, 0.0, 0.0], [10.0, 0.0, 0.0, 0.0]];
        let (_, deep_cls) = d.classify_points(&probe_pts, &torus_tris, &deep_bvh);
        assert_eq!(deep_cls, vec![false, true], "deep BVH classify mismatch on {target:?}");
    }
}

#[test]
fn collision_lbvh_morton_and_ssi_edge_cases() {
    use crate::collision::{GpuAabb, OverlapPair, build_simple_lbvh, cpu_bipartite_overlap, cpu_self_overlap};
    use crate::cuda::CudaBackend;
    use crate::dispatch::{Dispatcher, VulkanAccel, cpu_bipartite_overlap_rayon};
    use solidcraft_geom::Aabb3;

    let disp = Dispatcher::global();
    let cuda = CudaBackend::global();

    // 1. Outward-rounded GpuAabb::from_aabb3 conservativeness & Aabb3::EMPTY handling:
    let empty_gpu = GpuAabb::from_aabb3(&Aabb3::EMPTY, 0);
    let unit_aabb = Aabb3 { min: Vec3::new(0.1, 0.2, 0.3), max: Vec3::new(1.1, 1.2, 1.3) };
    let unit_gpu = GpuAabb::from_aabb3(&unit_aabb, 1);
    assert!(f64::from(unit_gpu.min_x) < unit_aabb.min.x && f64::from(unit_gpu.max_x) > unit_aabb.max.x);
    assert!(f64::from(unit_gpu.min_y) < unit_aabb.min.y && f64::from(unit_gpu.max_y) > unit_aabb.max.y);
    assert!(f64::from(unit_gpu.min_z) < unit_aabb.min.z && f64::from(unit_gpu.max_z) > unit_aabb.max.z);
    assert!(cpu_bipartite_overlap(&[empty_gpu], &[unit_gpu], 0.0).pairs.is_empty(), "Aabb3::EMPTY must not overlap finite box");

    // 2. Empty and single-element inputs across bipartite and self-overlap:
    assert!(cpu_bipartite_overlap(&[], &[unit_gpu], 0.0).pairs.is_empty());
    assert!(cpu_bipartite_overlap(&[unit_gpu], &[], 0.0).pairs.is_empty());
    assert!(cpu_bipartite_overlap_rayon(&[], &[unit_gpu], 0.0).pairs.is_empty());
    assert!(cpu_self_overlap(&[], 0.0).pairs.is_empty());
    assert!(cpu_self_overlap(&[unit_gpu], 0.0).pairs.is_empty());

    // 3. Exact tolerance boundary conditions:
    // Box A = [0, 1]^3, Box B = [1.02, 2.02] x [0, 1]^2 (gap along X is 0.02).
    // Overlap condition is `a.max_x + tol >= b.min_x`, so tol = 0.015 -> no overlap; tol = 0.025 -> overlap.
    // Touching box C = [1.0, 2.0] x [0, 1]^2 overlaps at tol = 0.0 and negative tol = -5.0 (clamped to 0.0).
    let box_a = GpuAabb { min_x: 0.0, min_y: 0.0, min_z: 0.0, id: 0, max_x: 1.0, max_y: 1.0, max_z: 1.0, pad: 0 };
    let box_b = GpuAabb { min_x: 1.02, min_y: 0.0, min_z: 0.0, id: 1, max_x: 2.02, max_y: 1.0, max_z: 1.0, pad: 0 };
    let box_c = GpuAabb { min_x: 1.0, min_y: 0.0, min_z: 0.0, id: 2, max_x: 2.0, max_y: 1.0, max_z: 1.0, pad: 0 };

    for target in BVH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        assert!(d.bipartite_aabb_overlap(&[box_a], &[box_b], 0.015).1.pairs.is_empty(), "gap=0.02 with tol=0.015 must not overlap on {target:?}");
        assert_eq!(
            d.bipartite_aabb_overlap(&[box_a], &[box_b], 0.025).1.pairs,
            vec![OverlapPair { a: 0, b: 1 }],
            "gap=0.02 with tol=0.025 must overlap on {target:?}"
        );
        assert_eq!(
            d.bipartite_aabb_overlap(&[box_a], &[box_c], 0.0).1.pairs,
            vec![OverlapPair { a: 0, b: 2 }],
            "touching boxes at tol=0.0 must overlap on {target:?}"
        );
        assert_eq!(
            d.bipartite_aabb_overlap(&[box_a], &[box_c], -5.0).1.pairs,
            vec![OverlapPair { a: 0, b: 2 }],
            "negative tol must clamp to 0.0 on {target:?}"
        );
    }

    // 4. Workgroup tile boundary sizes (1, 15, 16, 17, 31, 32, 33) for 16x16 bipartite and 256 self-overlap:
    for (na, nb) in [(1usize, 1usize), (15, 17), (16, 16), (17, 31), (32, 33)] {
        let set_a: Vec<GpuAabb> = (0..na)
            .map(|i| {
                let x = i as f32 * 0.75;
                GpuAabb { min_x: x, min_y: 0.0, min_z: 0.0, id: i as u32, max_x: x + 1.0, max_y: 1.0, max_z: 1.0, pad: 0 }
            })
            .collect();
        let set_b: Vec<GpuAabb> = (0..nb)
            .map(|j| {
                let x = j as f32 * 0.75 + 0.25;
                GpuAabb { min_x: x, min_y: 0.0, min_z: 0.0, id: j as u32, max_x: x + 1.0, max_y: 1.0, max_z: 1.0, pad: 0 }
            })
            .collect();
        let ref_bi = cpu_bipartite_overlap(&set_a, &set_b, 1e-4);
        let ref_self = cpu_self_overlap(&set_a, 1e-4);
        for pair in &ref_self.pairs {
            assert!(pair.a < pair.b, "self-overlap must strictly emit upper-triangular pairs (a < b)");
        }
        for target in COMPUTE_TARGETS {
            let (_, got_bi) = disp.with_forced_backend(Some(target)).bipartite_aabb_overlap(&set_a, &set_b, 1e-4);
            assert_eq!(got_bi.pairs, ref_bi.pairs, "bipartite tile boundary ({na}, {nb}) mismatch on {target:?}");
        }
        if let Some(vk) = VulkanAccel::global() {
            let got_self = vk.collision().self_overlap(&set_a, 1e-4).expect("vk self_overlap");
            assert_eq!(got_self.pairs, ref_self.pairs, "Vulkan self_overlap mismatch for na={na}");
        }
    }

    // 5. Coincident / zero-extent scene Morton codes, single-leaf LBVH traversal, & storage buffer limit validation:
    if let Some(vk) = VulkanAccel::global() {
        assert!(vk.collision().fits_storage(1024));
        assert!(!vk.collision().fits_storage(usize::MAX));
        let coincident_boxes = vec![box_a; 8];
        let morton = vk.collision().compute_morton(&coincident_boxes).expect("vk morton coincident");
        assert_eq!(morton.len(), 8);
        assert!(morton.iter().all(|m| m.code == 0), "zero-extent scene must produce Morton code 0 without NaN");
        let lbvh_coincident = build_simple_lbvh(&coincident_boxes, &morton);
        assert_eq!(lbvh_coincident.len(), 15);
        let res_coincident = vk.collision().lbvh_query(&[box_a], &lbvh_coincident, 0.0).expect("vk lbvh coincident");
        assert_eq!(res_coincident.pairs.len(), 8);

        let single_morton = vk.collision().compute_morton(&[box_a]).expect("vk morton single");
        let single_lbvh = build_simple_lbvh(&[box_a], &single_morton);
        assert_eq!(single_lbvh.len(), 1);
        let single_res = vk.collision().lbvh_query(&[box_a, box_b, box_c], &single_lbvh, 0.0).expect("vk lbvh single");
        assert_eq!(single_res.pairs, vec![OverlapPair { a: 0, b: 0 }, OverlapPair { a: 2, b: 0 }]);

        // Caller-supplied 65-leaf left-comb LBVH exceeding LBVH_STACK_CAP (64) must be rejected by lbvh_fits_traversal_stack and lbvh_query:
        assert!(crate::collision::lbvh_fits_traversal_stack(&lbvh_coincident));
        let mut left_comb = Vec::with_capacity(129);
        for d in 0..64u32 {
            left_comb.push(crate::collision::GpuLbvhNode {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                left_or_prim: if d < 63 { d + 1 } else { 64 },
                max_x: 1.0,
                max_y: 1.0,
                max_z: 1.0,
                right_or_flag: 65 + d,
            });
        }
        for leaf_id in 0..65u32 {
            left_comb.push(crate::collision::GpuLbvhNode {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                left_or_prim: leaf_id,
                max_x: 1.0,
                max_y: 1.0,
                max_z: 1.0,
                right_or_flag: 0x8000_0000,
            });
        }
        assert!(!crate::collision::lbvh_fits_traversal_stack(&left_comb));
        assert!(vk.collision().lbvh_query(&[box_a], &left_comb, 0.0).is_none());

        // Whereas a 65-leaf right-comb LBVH (left child is always a leaf, right child is internal)
        // keeps at most 2 pending nodes on the shader stack and must succeed with all 65 pairs:
        let mut right_comb = Vec::with_capacity(129);
        for d in 0..64u32 {
            right_comb.push(crate::collision::GpuLbvhNode {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                left_or_prim: 64 + d,
                max_x: 1.0,
                max_y: 1.0,
                max_z: 1.0,
                right_or_flag: if d < 63 { d + 1 } else { 128 },
            });
        }
        for leaf_id in 0..65u32 {
            right_comb.push(crate::collision::GpuLbvhNode {
                min_x: 0.0,
                min_y: 0.0,
                min_z: 0.0,
                left_or_prim: leaf_id,
                max_x: 1.0,
                max_y: 1.0,
                max_z: 1.0,
                right_or_flag: 0x8000_0000,
            });
        }
        assert!(crate::collision::lbvh_fits_traversal_stack(&right_comb));
        let right_res = vk.collision().lbvh_query(&[box_a], &right_comb, 0.0).expect("vk lbvh right_comb");
        assert_eq!(right_res.pairs.len(), 65);

        // 6. SSI surface residual evaluation: disjoint surfaces vs intersecting surfaces & grid_res clamping:
        let c1 = ok(solidcraft_kernel::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 5.0));
        let c2_disjoint = ok(solidcraft_kernel::cylinder(Vec3::new(20.0, 0.0, 0.0), Vec3::Z, 2.0, 5.0));
        let p1 = ok(c1.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.1)));
        let p2_disjoint = ok(c2_disjoint.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.1)));
        if let (Some(sa), Some(sb)) = (p1.surfaces.first(), p2_disjoint.surfaces.first()) {
            let disjoint_seeds = vk
                .collision()
                .surface_residuals(sa, (0.0, 1.0), (0.0, 1.0), sb, (0.0, 1.0), (0.0, 1.0), 8, 1.0)
                .expect("vk disjoint surface_residuals");
            assert!(disjoint_seeds.is_empty(), "disjoint surfaces separated by >15 units must return 0 seeds at threshold=1.0");

            // Same surface against itself with grid_res = 1 (clamped to 2) must find exact zero-distance seeds on the diagonal:
            let self_seeds = vk
                .collision()
                .surface_residuals(sa, (0.0, 1.0), (0.0, 1.0), sa, (0.0, 1.0), (0.0, 1.0), 1, 0.01)
                .expect("vk self surface_residuals grid_res=1");
            assert!(self_seeds.len() >= 4, "2x2 clamped grid self-surface residual must find at least 4 zero-distance seeds");
            assert!(self_seeds[0].dist_sq < 1e-10);
        }
    }
    let _ = cuda;
}

#[test]
fn point_mesh_distance_voronoi_and_edge_cases() {
    use crate::cuda::GpuTri;
    use crate::dispatch::{Dispatcher, cpu_point_mesh_distance_scalar};

    let disp = Dispatcher::global();

    // 1. Exact analytical verification of all 7 Voronoi feature regions on a single right triangle:
    // A = (0, 0, 0), B = (4, 0, 0), C = (0, 3, 0).
    let tri = GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [4.0, 0.0, 0.0, 0.0], v2: [0.0, 3.0, 0.0, 0.0] };
    let voronoi_cases: &[([f32; 4], f32, &str)] = &[
        // Vertex A region: closest is (0, 0, 0), d^2 = (-1)^2 + (-2)^2 + 2^2 = 9.0
        ([-1.0, -2.0, 2.0, 0.0], 9.0, "Vertex A Voronoi region"),
        // Vertex B region: closest is (4, 0, 0), d^2 = (6-4)^2 + (-1)^2 + 2^2 = 9.0
        ([6.0, -1.0, 2.0, 0.0], 9.0, "Vertex B Voronoi region"),
        // Vertex C region: closest is (0, 3, 0), d^2 = (-2)^2 + (5-3)^2 + 1^2 = 9.0
        ([-2.0, 5.0, 1.0, 0.0], 9.0, "Vertex C Voronoi region"),
        // Edge AB region: closest is (2, 0, 0), d^2 = 0^2 + (-3)^2 + 4^2 = 25.0
        ([2.0, -3.0, 4.0, 0.0], 25.0, "Edge AB Voronoi region"),
        // Edge AC region: closest is (0, 1.5, 0), d^2 = (-3)^2 + 0^2 + 4^2 = 25.0
        ([-3.0, 1.5, 4.0, 0.0], 25.0, "Edge AC Voronoi region"),
        // Edge BC region: (2, 1.5, 0) is midpoint of BC; outward normal in XY is (3, 4)/5.
        // P = (2, 1.5, 0) + (1.2, 1.6, 2.0) = (3.2, 3.1, 2.0) -> closest is (2, 1.5, 0), d^2 = 1.44 + 2.56 + 4.0 = 8.0
        ([3.2, 3.1, 2.0, 0.0], 8.0, "Edge BC Voronoi region"),
        // Face interior region: P = (1, 1, -5) -> closest is (1, 1, 0), d^2 = 25.0
        ([1.0, 1.0, -5.0, 0.0], 25.0, "Face interior Voronoi region"),
        // Point directly on triangle interior: P = (1, 1, 0) -> d^2 = 0.0
        ([1.0, 1.0, 0.0, 0.0], 0.0, "Point on triangle interior"),
        // Point directly on vertex B: P = (4, 0, 0) -> d^2 = 0.0
        ([4.0, 0.0, 0.0, 0.0], 0.0, "Point on vertex B"),
    ];
    let pts: Vec<[f32; 4]> = voronoi_cases.iter().map(|&(p, _, _)| p).collect();

    for target in COMPUTE_TARGETS {
        let (_, d2) = disp.with_forced_backend(Some(target)).point_mesh_distance(&pts, &[tri]);
        for (i, &(_, exp_d2, label)) in voronoi_cases.iter().enumerate() {
            assert!((d2[i] - exp_d2).abs() < 1e-4, "Voronoi distance mismatch for '{label}' on {target:?}: got {}, expected {exp_d2}", d2[i]);
        }
    }

    // 2. Degenerate triangles (coincident vertices A==B==C, A==B!=C, A==C!=B, B==C!=A, and collinear vertices):
    // Verify no backend ever emits NaN!
    let degen_tris = vec![
        GpuTri { v0: [1.0, 2.0, 3.0, 0.0], v1: [1.0, 2.0, 3.0, 0.0], v2: [1.0, 2.0, 3.0, 0.0] },
        GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [0.0, 0.0, 0.0, 0.0], v2: [2.0, 0.0, 0.0, 0.0] },
        GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [2.0, 0.0, 0.0, 0.0], v2: [0.0, 0.0, 0.0, 0.0] },
        GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [2.0, 0.0, 0.0, 0.0], v2: [2.0, 0.0, 0.0, 0.0] },
        GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [1.0, 0.0, 0.0, 0.0], v2: [2.0, 0.0, 0.0, 0.0] },
    ];
    for target in COMPUTE_TARGETS {
        let (_, d2) = disp.with_forced_backend(Some(target)).point_mesh_distance(&pts, &degen_tris);
        for (i, &val) in d2.iter().enumerate() {
            assert!(val.is_finite() && val >= 0.0, "degenerate triangle produced non-finite d2={val} at query {i} on {target:?}");
        }
    }

    // 3. Empty inputs and tile-boundary sizes (127, 128, 129 queries x 127, 128, 129 triangles):
    let mut many_tris = Vec::with_capacity(129);
    for i in 0..129 {
        let z = i as f32 * 0.5;
        many_tris.push(GpuTri { v0: [0.0, 0.0, z, 0.0], v1: [1.0, 0.0, z, 0.0], v2: [0.0, 1.0, z, 0.0] });
    }
    let mut many_pts = Vec::with_capacity(129);
    for i in 0..129 {
        many_pts.push([0.25, 0.25, i as f32 * 0.5 + 0.1, 0.0]);
    }
    let ref_many = cpu_point_mesh_distance_scalar(&many_pts, &many_tris);
    for target in COMPUTE_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        assert!(d.point_mesh_distance(&[], &many_tris).1.is_empty());
        let (_, empty_tri_d2) = d.point_mesh_distance(&pts, &[]);
        assert!(empty_tri_d2.iter().all(|&x| x.is_infinite() && x > 0.0));
        let (_, got_many) = d.point_mesh_distance(&many_pts, &many_tris);
        for i in 0..129 {
            assert!((got_many[i] - ref_many[i]).abs() < 1e-5, "tile-boundary 129x129 mismatch at {i} on {target:?}");
        }
    }
}

#[test]
fn mass_properties_common_and_edge_cases() {
    use crate::cuda::GpuTri;
    use crate::dispatch::{BackendTarget, Dispatcher, build_gpu_bvh_from_mesh};

    let disp = Dispatcher::global();

    // 1. Empty mesh -> all zeros without NaN:
    for target in COMPUTE_TARGETS {
        let (_, mp) = disp.with_forced_backend(Some(target)).mass_properties(&[]);
        assert_eq!(mp.volume, 0.0);
        assert_eq!(mp.area, 0.0);
        assert_eq!(mp.centroid, Vec3::ZERO);
    }

    // 2. Open zero-volume flat 2x3 rectangle in the z = 0 plane:
    // Volume = 0.0, Surface Area = 6.0, Centroid = Vec3::ZERO (guarded against division by zero v6 == 0).
    let flat_sheet = vec![
        GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [2.0, 0.0, 0.0, 0.0], v2: [2.0, 3.0, 0.0, 0.0] },
        GpuTri { v0: [0.0, 0.0, 0.0, 0.0], v1: [2.0, 3.0, 0.0, 0.0], v2: [0.0, 3.0, 0.0, 0.0] },
    ];
    for target in COMPUTE_TARGETS {
        let (_, mp) = disp.with_forced_backend(Some(target)).mass_properties(&flat_sheet);
        assert!(mp.volume.abs() < 1e-12, "flat sheet volume must be 0 on {target:?}");
        assert!((mp.area - 6.0).abs() < 1e-5, "flat sheet area must be 6.0 on {target:?}");
        assert_eq!(mp.centroid, Vec3::ZERO, "zero-volume flat sheet centroid must be Vec3::ZERO on {target:?}");
    }

    // 3. Torus shifted away from origin at (5, -3, 2) with R = 8, r = 2 (> 512 triangles across multiple 256-thread workgroups):
    // Analytical volume = 2 * pi^2 * R * r^2 = 64 * pi^2 ~= 631.65468
    // Analytical area   = 4 * pi^2 * R * r   = 64 * pi^2 ~= 631.65468
    let center = Vec3::new(5.0, -3.0, 2.0);
    let torus_solid = ok(solidcraft_kernel::torus(center, 8.0, 2.0));
    let torus_mesh = ok(torus_solid.mesh(&solidcraft_kernel::MeshTolerance::chord(0.05)));
    let expected_mp = torus_mesh.measure();
    let (torus_tris, _) = build_gpu_bvh_from_mesh(&torus_mesh);
    assert!(torus_tris.len() > 512, "torus mesh must span multiple 256-thread reduction workgroups");

    for target in COMPUTE_TARGETS {
        let (_, mp) = disp.with_forced_backend(Some(target)).mass_properties(&torus_tris);
        let rel_vol = (mp.volume - expected_mp.volume).abs() / expected_mp.volume;
        let rel_area = (mp.area - expected_mp.area).abs() / expected_mp.area;
        let tol = if target == BackendTarget::VulkanCompute { 1e-4 } else { 1e-6 };
        assert!(rel_vol < tol, "torus volume mismatch on {target:?}: got {}, expected {}", mp.volume, expected_mp.volume);
        assert!(rel_area < tol, "torus area mismatch on {target:?}: got {}, expected {}", mp.area, expected_mp.area);
        assert!(mp.centroid.dist(expected_mp.centroid) < 1e-3, "torus centroid mismatch on {target:?}");
    }

    // 4. Inverted winding orientation: negates signed volume, preserves area and centroid:
    let inverted_tris: Vec<GpuTri> = torus_tris.iter().map(|t| GpuTri { v0: t.v0, v1: t.v2, v2: t.v1 }).collect();
    for target in COMPUTE_TARGETS {
        let (_, mp_inv) = disp.with_forced_backend(Some(target)).mass_properties(&inverted_tris);
        let tol = if target == BackendTarget::VulkanCompute { 1e-4 } else { 1e-6 };
        assert!((mp_inv.volume + expected_mp.volume).abs() / expected_mp.volume < tol, "inverted volume must be -V on {target:?}");
        assert!((mp_inv.area - expected_mp.area).abs() / expected_mp.area < tol, "inverted area must equal +A on {target:?}");
        assert!(mp_inv.centroid.dist(expected_mp.centroid) < 1e-3, "inverted centroid must be preserved on {target:?}");
    }
}
