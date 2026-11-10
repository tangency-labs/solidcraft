use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;
use solidcraft_geom::tess::EvaluateMesh as _;
use solidcraft_kernel::{Body, BoolOp};

use solidcore_tess::EvalSurface;
use solidcore_tess::TessPlan;
use solidcraft_viewport_gpu::{GpuError, PackedPlans, SHADER, TessGpu, VERTEX_STRIDE, open_headless};

/// The adapter policy this crate's GPU tests run under: the host's, from its environment.
pub(crate) fn policy() -> solidcraft_viewport_gpu::select::Policy {
    crate::env::policy().unwrap_or_else(|e| panic!("{e}"))
}

/// The kernel's plan; the GPU evaluates it in solidcore-tess's form, made by the model.
type GeomPlan = solidcraft_geom::tess::TessPlan;

/// `plan` as the model hands it to the GPU ([`solidcraft_engine::doc::gpu_plan`]).
fn gpu_form(plan: &GeomPlan) -> TessPlan {
    solidcraft_engine::doc::gpu_plan::gpu_plan(plan)
}

/// The GPU, or `None` with a note when the machine has no adapter (CI runners without a GPU).
/// A device that cannot build the pipeline fails the test: that is a shader the device rejects
/// (as D3D12's FXC once did), not a missing GPU. `SOLIDCRAFT_REQUIRE_GPU=1` (the Windows CI job,
/// on the WARP software adapter) fails without an adapter too.
fn gpu() -> Option<TessGpu> {
    match TessGpu::headless(crate::tests::policy()) {
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
    let (adapter, device, queue) = match open_headless(crate::tests::policy()) {
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
    assert_eq!(ok(gpu.evaluate(&gpu_form(&plan))).positions.len(), plan.evaluate_mesh().positions.len());
}

/// The shader as the D3D12 backend translates it (naga's HLSL for shader model 5.1, the FXC
/// target, with wgpu's options) avoids what FXC cannot compile without unrolling a loop:
/// arrays inside structs and two-dimensional arrays written by a loop counter. Runs everywhere;
/// the Windows CI job compiles the real thing with FXC.
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
    assert!(shaders.iter().any(|p| p.ends_with("crates/viewport-gpu/shaders/viewport.wgsl")), "{shaders:?}");
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
        let cpu = plan.evaluate_mesh();
        let got = ok(gpu.evaluate(&gpu_form(&plan)));
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
        let cpu = plan.evaluate_mesh();
        let mesh = ok(gpu.prepare(&gpu_form(&plan)));
        assert_eq!(mesh.vertex_count as usize, cpu.positions.len(), "{name}");
        assert_eq!(mesh.index_count as usize, cpu.triangles.len() * 3, "{name}");
        assert_eq!(mesh.vertex_face.len(), cpu.positions.len(), "{name}");
        for (t, f) in cpu.triangles.iter().zip(&cpu.tri_face) {
            assert!(t.iter().all(|&i| mesh.vertex_face[i as usize] == *f), "{name}: a triangle's vertices belong to its face");
        }
        let d = gpu.device();
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
            gpu.queue().submit([enc.finish()]);
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
    let plans: Vec<GeomPlan> = bodies()
        .iter()
        .map(|(_, b)| ok(b.mesh_plan(&solidcraft_kernel::MeshTolerance { chord: 0.01, angle: Some(std::f64::consts::PI / 9.0) })))
        .collect();
    let gpu_plans: Vec<TessPlan> = plans.iter().map(gpu_form).collect();
    let refs: Vec<&TessPlan> = gpu_plans.iter().collect();
    let mesh = ok(gpu.prepare_many(&refs));
    assert_eq!(mesh.parts.len(), plans.len());
    let d = gpu.device();
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
    gpu.queue().submit([enc.finish()]);
    let slice = dst.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    ok(d.poll(wgpu::PollType::wait_indefinitely()));
    let out: Vec<f32> = ok(slice.get_mapped_range()).as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    for ((plan, part), (name, _)) in plans.iter().zip(&mesh.parts).zip(bodies()) {
        let cpu = plan.evaluate_mesh();
        assert_eq!((part.vertices.end - part.vertices.start) as usize, cpu.positions.len(), "{name}");
        assert_eq!((part.indices.end - part.indices.start) as usize, cpu.triangles.len() * 3, "{name}");
        for (k, p) in cpu.positions.iter().enumerate() {
            let i = part.vertices.start as usize + k;
            let q = Vec3::new(f64::from(out[8 * i]), f64::from(out[8 * i + 1]), f64::from(out[8 * i + 2]));
            assert!(q.dist(*p) < 20.0 * 1e-5, "{name}: vertex {k}");
        }
    }
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
            let m = plan.evaluate_mesh();
            c.push(ms(t));
            np = m.triangles.len();
            if let Some(gpu) = &gpu {
                let t = std::time::Instant::now();
                let _ = ok(gpu.evaluate(&gpu_form(&plan)));
                g.push(ms(t));
            }
        }
        let g = if g.is_empty() { "-".to_string() } else { format!("{:.1} ms", median(g)) };
        println!("| {tol} | {:.1} ms | {:.1} ms | {:.1} ms | {g} | {nr} / {np} |", median(r), median(p), median(c));
    }
}

/// Plans that use every packed field: a plane face with a pinned node, a flipped rational NURBS
/// face closed along u, and a revolved face with its parameters swapped under a mirroring transform.
fn synthetic_plans() -> Vec<GeomPlan> {
    use solidcraft_geom::tess::{EvalCurve, EvalSurface, FacePlan, to_kernel};
    let plane = EvalSurface::Plane {
        origin: to_kernel(Vec3::new(1.0, 2.0, 3.0)),
        du: to_kernel(Vec3::new(0.5, 0.0, 0.0)),
        dv: to_kernel(Vec3::new(0.0, 0.25, 0.0)),
    };
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let ctrl = vec![[1.0, 0.0, 0.0, 1.0], [w, w, 0.0, w], [0.0, 1.0, 0.0, 1.0], [1.0, 0.0, 2.0, 1.0], [w, w, 2.0 * w, w], [0.0, 1.0, 2.0, 1.0]];
    let nurbs = EvalSurface::Nurbs {
        degree: [2, 1],
        knots: [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]],
        count: [3, 2],
        ctrl,
        closed: [true, false],
    };
    let profile = EvalCurve { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], ctrl: vec![[2.0, 0.0, 0.0, 1.0], [3.0, 0.0, 1.5, 1.0]] };
    let revolved = EvalSurface::Revolved {
        profile,
        origin: to_kernel(Vec3::new(0.0, 0.0, -1.0)),
        axis: to_kernel(Vec3::Z),
        transform: [[-1.0, 0.0, 0.0, 4.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.5]],
        swap: true,
    };
    let face = |face: u32, surface: u32, flip: bool, fixed: Vec<Vec3>| FacePlan {
        face,
        surface: Some(surface),
        uv: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [0.75, 0.5]],
        fixed,
        normals: Vec::new(),
        flip,
        triangles: vec![[0, 1, 2], [1, 3, 2]],
        poles: Vec::new(),
    };
    vec![
        GeomPlan {
            surfaces: vec![plane, nurbs],
            faces: vec![face(0, 0, false, vec![Vec3::new(1.0, 2.0, 3.0)]), face(1, 1, true, Vec::new())],
            edges: vec![vec![Vec3::new(1.0, 2.0, 3.0), Vec3::new(1.5, 2.0, 3.0)]],
            edge_faces: vec![vec![0]],
        },
        GeomPlan { surfaces: vec![revolved], faces: vec![face(0, 0, false, Vec::new())], edges: Vec::new(), edge_faces: Vec::new() },
    ]
}

/// FNV-1a over what the shader reads of packed plans (surface headers, data, vertices) and what
/// the blocking evaluation restores (the pinned nodes), so packing is pinned byte for byte.
fn packed_digest(plans: &[&TessPlan]) -> u64 {
    let packed = ok(PackedPlans::of(plans));
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h = (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for w in &packed.surfaces {
        eat(&w.to_le_bytes());
    }
    for x in &packed.data {
        eat(&x.to_bits().to_le_bytes());
    }
    for w in &packed.vertices {
        eat(&w.to_le_bytes());
    }
    eat(&(packed.count as u64).to_le_bytes());
    for (k, p) in &packed.fixed {
        eat(&(*k as u64).to_le_bytes());
        for c in [p.x, p.y, p.z] {
            eat(&c.to_bits().to_le_bytes());
        }
    }
    h
}

/// What the shader is given, byte for byte, for plans built from literal numbers: one of each
/// surface kind, alone and packed together. A kernel-made body's plan rounds differently on
/// x86-64 and arm64, so real bodies are only required to pack, and to pack the same way twice.
#[test]
fn plans_pack_to_the_same_bytes() {
    let tol = solidcraft_kernel::MeshTolerance::chord(0.05);
    let real: Vec<TessPlan> = bodies().into_iter().map(|(_, b)| gpu_form(&ok(b.mesh_plan(&tol)))).collect();
    for plan in &real {
        assert_eq!(packed_digest(&[plan]), packed_digest(&[plan]), "packing a plan twice gives the same bytes");
    }
    let synthetic: Vec<TessPlan> = synthetic_plans().iter().map(gpu_form).collect();
    let kinds = |k: fn(&EvalSurface) -> bool| synthetic.iter().flat_map(|p| &p.surfaces).filter(|s| k(s)).count();
    assert!(kinds(|s| matches!(s, EvalSurface::Plane { .. })) > 0);
    assert!(kinds(|s| matches!(s, EvalSurface::Nurbs { .. })) > 0 && kinds(|s| matches!(s, EvalSurface::Revolved { .. })) > 0);
    let digests: Vec<u64> = synthetic.iter().map(|p| packed_digest(&[p])).collect();
    let together = packed_digest(&synthetic.iter().collect::<Vec<_>>());
    assert_eq!((digests, together), (vec![5379207237662626536, 16919141291401223349], 4471009163941807655));
}

/// A plan with pole nodes (one vertex per incident triangle, its own normal) is refused by name:
/// the shader has no rule for them, so the body is drawn from its CPU mesh as any refused plan is.
#[test]
fn a_plan_with_pole_nodes_is_refused_by_name() {
    let mut plan = gpu_form(&synthetic_plans()[0]);
    let face = plan.faces.get_mut(1).map(|f| {
        f.poles.push(solidcore_tess::PoleNode { node: 0, rule: solidcore_tess::PoleRule::Averaged });
        f.face
    });
    assert_eq!(face, Some(1));
    assert!(matches!(PackedPlans::of(&[&plan]), Err(GpuError::PoleNodes { face: 1 })));
    let Some(gpu) = gpu() else { return };
    assert!(matches!(gpu.prepare(&plan), Err(GpuError::PoleNodes { face: 1 })));
    assert!(matches!(gpu.evaluate(&plan), Err(GpuError::PoleNodes { face: 1 })));
}

/// Verifies numerical parity of `f32` bicubic Bézier tensor-product patch evaluation across CPU, Vulkan, CUDA PTX, and Ampere MMA Tensor Cores.
#[test]
fn multi_backend_bicubic_patch_f32_and_tensor_mma_parity() {
    use crate::cuda::{CudaBackend, PatchUvQueryF32};
    use crate::dispatch::{VulkanAccel, cpu_eval_bicubic_f32_rayon, cpu_eval_bicubic_f32_scalar, synthetic_bicubic_patch_f32};

    let patch_f32 = synthetic_bicubic_patch_f32();

    // Use 259 queries (non-multiple of 4 and 64) to exercise tail handling in Tensor Core MMA and workgroups.
    let n = 259usize;
    let queries_f32: Vec<PatchUvQueryF32> = (0..n)
        .map(|i| {
            let t = (i as f32) / (n as f32);
            PatchUvQueryF32 { u: (t * 0.89 + 0.05).fract(), v: (t * 2.71 + 0.07).fract(), patch_idx: 0, pad: 0 }
        })
        .collect();

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

        let cu_tensor_gen = cuda.eval_bicubic_tensor_f32(&patch_f32, &queries_f32, false).expect("cuda generic tensor f32");
        for (i, (a, b)) in ref_f32.iter().zip(&cu_tensor_gen).enumerate() {
            for c in 0..3 {
                assert!((a.pos[c] - b.pos[c]).abs() < 1e-4, "cuda sm70 tensor f32 pos mismatch at {i}[{c}]: {} vs {}", a.pos[c], b.pos[c]);
                assert!((a.norm[c] - b.norm[c]).abs() < 1e-4, "cuda sm70 tensor f32 norm mismatch at {i}[{c}]: {} vs {}", a.norm[c], b.norm[c]);
            }
        }

        if cuda.has_f32_tensor_core() {
            let cu_f32_mma = cuda.eval_bicubic_tensor_f32(&patch_f32, &queries_f32, true).expect("cuda sm86 MMA tensor core f32");
            for (i, (a, b)) in ref_f32.iter().zip(&cu_f32_mma).enumerate() {
                for c in 0..3 {
                    assert!(
                        (a.pos[c] - b.pos[c]).abs() < 1e-5,
                        "Ampere MMA f32 pos mismatch at query {i} comp {c}: cpu={} mma={}",
                        a.pos[c],
                        b.pos[c]
                    );
                    assert!(
                        (a.norm[c] - b.norm[c]).abs() < 1e-5,
                        "Ampere MMA f32 norm mismatch at query {i} comp {c}: cpu={} mma={}",
                        a.norm[c],
                        b.norm[c]
                    );
                }
            }
        }
    }
}

/// Verifies that CUDA device info enables hardware F32 tensor cores (`mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32`)
/// across all consumer and datacenter Ampere (`sm_80+`) and newer architectures.
#[test]
fn cuda_f32_tensor_core_detection_rules() {
    use crate::cuda::{CudaBackend, CudaDeviceInfo};
    if let Some(cuda) = CudaBackend::global() {
        let dev = cuda.device_info();
        let (major, minor) = dev.compute_capability;
        let expected_hw_f32 = major >= 8;
        assert_eq!(
            dev.supports_f32_tensor_core, expected_hw_f32,
            "device {} (SM {}.{}) F32 tensor core flag must be {} (hardware F32 MMA is supported on sm_80+)",
            dev.name, major, minor, expected_hw_f32
        );
        assert_eq!(cuda.has_f32_tensor_core(), expected_hw_f32 && cuda.has_ampere_ptx());
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
        supports_f32_tensor_core: cc.0 >= 8,
        supports_rt_core: cc >= (7, 5),
    };
    let pascal_only = [mk_dev(0, (6, 1), 28, 11 << 30), mk_dev(1, (0, 0), 64, 16 << 30)];
    assert!(CudaBackend::select_best_device(&pascal_only).is_none());

    let mixed = [mk_dev(0, (6, 1), 28, 11 << 30), mk_dev(1, (8, 6), 20, 8 << 30), mk_dev(2, (7, 5), 20, 16 << 30)];
    assert_eq!(CudaBackend::select_best_device(&mixed).expect("eligible gpu").ordinal, 1);
}

/// Verifies [`Dispatcher`] multi-sample rotated calibration, markdown reporting, `GpuPod` codec, and `evaluate_tess_plan`.
#[test]
fn dispatcher_auto_tuning_calibration_and_routing() {
    use crate::collision::{
        GpuAabb, GpuPod, OverlapPair, aabbs_from_bounds, decode_pod_slice, encode_pod, encode_pod_slice, face_aabbs_from_mesh, face_aabbs_from_plan,
        synthetic_aabb_grid, triangle_aabbs_from_mesh,
    };
    use crate::dispatch::{
        BENCH_SIZES, BackendTarget, CALIBRATION_SAMPLES, Dispatcher, KernelCalibration, KernelOp, SizeBucketCalibration, synthetic_bicubic_patch_f32,
        synthetic_closed_sphere_mesh,
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
    assert!(md.contains("SolidCraft Multi-Backend GPU & Auto-Tuning Dispatcher Report"));
    assert!(md.contains("Bicubic Tensor Patch Eval (f32 MMA)"));

    let _ = synthetic_bicubic_patch_f32();
    let _ = synthetic_closed_sphere_mesh(5.0, 6, 6);
    let _ = synthetic_aabb_grid(16, 2.0, 0.8);

    assert!(decode_pod_slice::<GpuAabb>(&[1, 2, 3]).is_empty(), "truncated byte slice (< 1 word) must decode to empty");
    assert!(decode_pod_slice::<GpuAabb>(&[0u8; 28]).is_empty(), "partial 7-word slice for 8-word GpuAabb must decode to empty");
    let empty_encoded = encode_pod_slice::<OverlapPair>(&[]);
    assert_eq!(empty_encoded.len(), OverlapPair::WORDS * 4, "encoding empty slice pads to 1 default element");
    assert_eq!(decode_pod_slice::<OverlapPair>(&empty_encoded), vec![OverlapPair::default()]);
    assert_eq!(encode_pod(&42u32), 42u32.to_le_bytes());

    let bounds = [(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 2.0, 3.0)), (Vec3::new(-1.0, -2.0, -3.0), Vec3::new(0.5, 0.5, 0.5))];
    let from_bounds = aabbs_from_bounds(&bounds);
    assert_eq!(from_bounds.len(), 2);
    assert!(from_bounds[0].overlaps(&from_bounds[1], 0.0));

    let box_body = ok(solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(4.0, 5.0, 6.0)));
    let box_mesh = ok(box_body.mesh(&solidcraft_kernel::MeshTolerance::chord(0.1)));
    let face_boxes = face_aabbs_from_mesh(&box_mesh);
    let tri_boxes = triangle_aabbs_from_mesh(&box_mesh);
    assert_eq!(face_boxes.len(), 6, "box mesh must have 6 face AABBs");
    assert_eq!(tri_boxes.len(), box_mesh.triangles.len());

    let small_plan = ok(box_body.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.1)));
    assert_eq!(face_aabbs_from_plan(&small_plan).len(), 6);
    assert!(small_plan.vertex_count() < 512);
    let small_eval = disp.evaluate_tess_plan(&small_plan);
    let small_ref = small_plan.evaluate_mesh();
    assert_eq!(small_eval.positions, small_ref.positions);
    assert_eq!(small_eval.triangles, small_ref.triangles);

    let fine_torus = ok(solidcraft_kernel::torus(Vec3::ZERO, 10.0, 3.0));
    let large_plan = ok(fine_torus.mesh_plan(&solidcraft_kernel::MeshTolerance::chord(0.02)));
    assert!(large_plan.vertex_count() >= 512);
    let large_eval = disp.evaluate_tess_plan(&large_plan);
    let large_ref = large_plan.evaluate_mesh();
    assert_eq!(large_eval.positions, large_ref.positions);
    assert_eq!(large_eval.normals, large_ref.normals);
    assert_eq!(large_eval.triangles, large_ref.triangles);

    let empty_cal = KernelCalibration {
        op: KernelOp::SurfaceEvalF32,
        buckets: Vec::new(),
        rayon_crossover_size: None,
        gpu_crossover_size: None,
        preferred_gpu_backend: None,
    };
    assert_eq!(empty_cal.select_backend(100), BackendTarget::CpuScalar);

    let mk_bucket = |size: usize, winner: BackendTarget| SizeBucketCalibration {
        problem_size: size,
        timings: Vec::new(),
        sample_orders: Vec::new(),
        sample_timings_ns: Vec::new(),
        fastest_backend: winner,
        fastest_latency_ns: 1,
        speedup_vs_scalar: 1.0,
    };
    let custom_cal = KernelCalibration {
        op: KernelOp::SurfaceEvalF32,
        buckets: vec![
            mk_bucket(64, BackendTarget::CpuScalar),
            mk_bucket(1024, BackendTarget::CpuRayon),
            mk_bucket(16384, BackendTarget::VulkanCompute),
        ],
        rayon_crossover_size: Some(1024),
        gpu_crossover_size: Some(16384),
        preferred_gpu_backend: Some(BackendTarget::VulkanCompute),
    };
    assert_eq!(custom_cal.select_backend(0), BackendTarget::CpuScalar);
    assert_eq!(custom_cal.select_backend(64), BackendTarget::CpuScalar);
    assert_eq!(custom_cal.select_backend(65), BackendTarget::CpuRayon);
    assert_eq!(custom_cal.select_backend(1024), BackendTarget::CpuRayon);
    assert_eq!(custom_cal.select_backend(1025), BackendTarget::VulkanCompute);
    assert_eq!(custom_cal.select_backend(1_000_000), BackendTarget::VulkanCompute);
    assert_eq!(
        disp.with_forced_backend(Some(BackendTarget::VulkanCompute)).select_backend(KernelOp::SurfaceEvalF32, 16),
        BackendTarget::VulkanCompute
    );
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
#[test]
fn bicubic_patch_common_and_edge_cases() {
    use crate::cuda::{CudaBackend, PatchUvQueryF32};
    use crate::dispatch::{Dispatcher, cpu_eval_bicubic_f32_rayon, cpu_eval_bicubic_f32_scalar, synthetic_bicubic_patch_f32};

    let disp = Dispatcher::global();
    let cuda = CudaBackend::global();

    // 1. Exact corner interpolation on synthetic rational bicubic patch:
    // At (u, v) in {(0,0), (1,0), (0,1), (1,1)}, S(u, v) = P_{3u, 3v} / w_{3u, 3v}.
    let cpts_f32 = synthetic_bicubic_patch_f32();
    let corners = [(0.0f32, 0.0f32, 0usize, 0usize), (1.0, 0.0, 3, 0), (0.0, 1.0, 0, 3), (1.0, 1.0, 3, 3)];
    let corner_queries_f32: Vec<PatchUvQueryF32> = corners.iter().map(|&(u, v, _, _)| PatchUvQueryF32 { u, v, patch_idx: 0, pad: 0 }).collect();

    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, evals_tensor) = d.eval_bicubic_tensor_f32(&cpts_f32, &corner_queries_f32);
        let (_, evals_f32) = d.eval_bicubic_f32(&cpts_f32, &corner_queries_f32);
        for (idx, &(_, _, i, j)) in corners.iter().enumerate() {
            let cpt = cpts_f32[i * 4 + j];
            let expected = [cpt[0] / cpt[3], cpt[1] / cpt[3], cpt[2] / cpt[3]];
            for (k, &exp) in expected.iter().enumerate() {
                assert!(
                    (evals_tensor[idx].pos[k] - exp).abs() < 1e-5,
                    "tensor f32 corner {idx} axis {k} mismatch on {target:?}: got {}, expected {exp}",
                    evals_tensor[idx].pos[k],
                );
                assert!(
                    (evals_f32[idx].pos[k] - exp).abs() < 1e-5,
                    "f32 corner {idx} axis {k} mismatch on {target:?}: got {}, expected {exp}",
                    evals_f32[idx].pos[k],
                );
            }
            let n_len = (evals_tensor[idx].norm[0].powi(2) + evals_tensor[idx].norm[1].powi(2) + evals_tensor[idx].norm[2].powi(2)).sqrt();
            assert!((n_len - 1.0).abs() < 1e-5, "unit normal required at corner {idx} on {target:?}");
            assert!(evals_tensor[idx].norm[3] > 0.0, "positive |Su x Sv| required at corner {idx} on {target:?}");
        }
    }

    // 2. Analytical planar patch S(u, v) = (2u + 1, 3v - 2, 5):
    // Control points at (i/3, j/3) with weight 1 -> dS/du = (2, 0, 0), dS/dv = (0, 3, 0),
    // Su x Sv = (0, 0, 6) -> unit normal = (0, 0, 1), |Su x Sv| = 6.0 everywhere.
    let mut planar_f32 = Vec::with_capacity(16);
    for i in 0..4 {
        for j in 0..4 {
            let u = (i as f32) / 3.0;
            let v = (j as f32) / 3.0;
            let x = 2.0 * u + 1.0;
            let y = 3.0 * v - 2.0;
            let z = 5.0;
            planar_f32.push([x, y, z, 1.0]);
        }
    }
    let sample_uvs = [(0.0f32, 0.0f32), (0.5, 0.0), (0.0, 0.5), (0.3, 0.7), (0.5, 0.5), (1.0, 1.0)];
    let planar_q32: Vec<PatchUvQueryF32> = sample_uvs.iter().map(|&(u, v)| PatchUvQueryF32 { u, v, patch_idx: 0, pad: 0 }).collect();

    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, out_tc) = d.eval_bicubic_tensor_f32(&planar_f32, &planar_q32);
        let (_, out32) = d.eval_bicubic_f32(&planar_f32, &planar_q32);
        for (idx, &(u, v)) in sample_uvs.iter().enumerate() {
            let exp_x = 2.0 * u + 1.0;
            let exp_y = 3.0 * v - 2.0;
            assert!((out_tc[idx].pos[0] - exp_x).abs() < 1e-5);
            assert!((out_tc[idx].pos[1] - exp_y).abs() < 1e-5);
            assert!((out_tc[idx].pos[2] - 5.0).abs() < 1e-5);
            assert!(out_tc[idx].norm[0].abs() < 1e-5);
            assert!(out_tc[idx].norm[1].abs() < 1e-5);
            assert!((out_tc[idx].norm[2] - 1.0).abs() < 1e-5);
            assert!((out_tc[idx].norm[3] - 6.0).abs() < 1e-4);

            assert!((out32[idx].pos[0] - exp_x).abs() < 1e-5);
            assert!((out32[idx].pos[1] - exp_y).abs() < 1e-5);
            assert!((out32[idx].pos[2] - 5.0).abs() < 1e-5);
            assert!((out32[idx].norm[2] - 1.0).abs() < 1e-5);
            assert!((out32[idx].norm[3] - 6.0).abs() < 1e-4);
        }
    }

    // 3. Multi-patch batching (patch_idx = 0, 1, 2) and out-of-bounds patch_idx clamping (patch_idx = 999 -> 2):
    let mut multi_f32 = Vec::with_capacity(48);
    for p_idx in 0..3 {
        let z_offset = (p_idx as f32) * 10.0;
        for cpt in &cpts_f32 {
            let w = cpt[3];
            multi_f32.push([cpt[0], cpt[1], cpt[2] + z_offset * w, w]);
        }
    }
    let multi_q32 = vec![
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 0, pad: 0 },
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 1, pad: 0 },
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 2, pad: 0 },
        PatchUvQueryF32 { u: 0.4, v: 0.6, patch_idx: 999, pad: 0 },
    ];
    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, mtc) = d.eval_bicubic_tensor_f32(&multi_f32, &multi_q32);
        let (_, m32) = d.eval_bicubic_f32(&multi_f32, &multi_q32);
        assert!((mtc[1].pos[2] - (mtc[0].pos[2] + 10.0)).abs() < 1e-4, "tensor patch 1 z shift failed on {target:?}");
        assert!((mtc[2].pos[2] - (mtc[0].pos[2] + 20.0)).abs() < 1e-4, "tensor patch 2 z shift failed on {target:?}");
        assert!((mtc[3].pos[2] - mtc[2].pos[2]).abs() < 1e-5, "out-of-bounds patch_idx must clamp to last patch on {target:?}");
        assert!((m32[1].pos[2] - (m32[0].pos[2] + 10.0)).abs() < 1e-4, "f32 patch 1 z shift failed on {target:?}");
        assert!((m32[2].pos[2] - (m32[0].pos[2] + 20.0)).abs() < 1e-4, "f32 patch 2 z shift failed on {target:?}");
        assert!((m32[3].pos[2] - m32[2].pos[2]).abs() < 1e-5, "f32 out-of-bounds patch_idx must clamp on {target:?}");
    }

    // 4. Degenerate singular patch (all 16 control points collapsed to a single point (3, -4, 7, 1)):
    // Derivatives Su and Sv are zero, so Su x Sv = 0. Must not produce NaN; fallback normal is [0, 0, 1, 0].
    let singular_f32 = vec![[3.0f32, -4.0, 7.0, 1.0]; 16];
    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, stc) = d.eval_bicubic_tensor_f32(&singular_f32, &[PatchUvQueryF32 { u: 0.5, v: 0.5, patch_idx: 0, pad: 0 }]);
        let (_, s32) = d.eval_bicubic_f32(&singular_f32, &[PatchUvQueryF32 { u: 0.5, v: 0.5, patch_idx: 0, pad: 0 }]);
        assert!((stc[0].pos[0] - 3.0).abs() < 1e-5 && (stc[0].pos[1] + 4.0).abs() < 1e-5 && (stc[0].pos[2] - 7.0).abs() < 1e-5);
        assert_eq!(stc[0].norm, [0.0, 0.0, 1.0, 0.0], "singular patch fallback normal mismatch on {target:?}");
        assert!((s32[0].pos[0] - 3.0).abs() < 1e-5 && (s32[0].pos[1] + 4.0).abs() < 1e-5 && (s32[0].pos[2] - 7.0).abs() < 1e-5);
        assert_eq!(s32[0].norm, [0.0, 0.0, 1.0, 0.0], "f32 singular patch fallback normal mismatch on {target:?}");
    }

    // 4b. Zero homogeneous weight (`w = 0` across all 16 control points):
    // `h[3] == 0.0` triggers the `inv_w = 1.0` guard (`|h[3]| <= 1e-12` in f32)
    // and must never divide by zero or emit NaN across CPU, Vulkan, CUDA, or MMA Tensor Core backends.
    let zero_w_f32: Vec<[f32; 4]> = cpts_f32.iter().map(|&[x, y, z, _]| [x, y, z, 0.0]).collect();
    let ref_zw32 = cpu_eval_bicubic_f32_scalar(&zero_w_f32, &[PatchUvQueryF32 { u: 0.3, v: 0.7, patch_idx: 0, pad: 0 }]);
    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, zwtc) = d.eval_bicubic_tensor_f32(&zero_w_f32, &[PatchUvQueryF32 { u: 0.3, v: 0.7, patch_idx: 0, pad: 0 }]);
        let (_, zw32) = d.eval_bicubic_f32(&zero_w_f32, &[PatchUvQueryF32 { u: 0.3, v: 0.7, patch_idx: 0, pad: 0 }]);
        for k in 0..4 {
            assert!(zwtc[0].pos[k].is_finite() && zwtc[0].norm[k].is_finite(), "tensor f32 zero-weight patch produced NaN/Inf on {target:?}");
            assert!(zw32[0].pos[k].is_finite() && zw32[0].norm[k].is_finite(), "f32 zero-weight patch produced NaN/Inf on {target:?}");
        }
        for k in 0..3 {
            assert!((zwtc[0].pos[k] - ref_zw32[0].pos[k]).abs() < 1e-4, "tensor f32 zero-weight pos[{k}] mismatch on {target:?}");
            assert!((zw32[0].pos[k] - ref_zw32[0].pos[k]).abs() < 1e-5, "f32 zero-weight pos[{k}] mismatch on {target:?}");
        }
    }

    // 4c. Strongly non-unit rational weights (`w` varying from 0.25 to 8.0) verifying the full
    // rational quotient rule `(dhu - P * dhw) / w` and `(dhv - P * dhw) / w` across all backends:
    let rational_curved_f32: Vec<[f32; 4]> = cpts_f32
        .iter()
        .enumerate()
        .map(|(idx, &[x, y, z, _])| {
            let w = 0.5 + ((idx * 7) % 11) as f32 * 0.35;
            [x, y, z + (idx as f32) * 0.2, w]
        })
        .collect();
    let rat_q32 = vec![
        PatchUvQueryF32 { u: 0.2, v: 0.8, patch_idx: 0, pad: 0 },
        PatchUvQueryF32 { u: 0.65, v: 0.35, patch_idx: 0, pad: 0 },
        PatchUvQueryF32 { u: 0.9, v: 0.1, patch_idx: 0, pad: 0 },
    ];
    let ref_rat32 = cpu_eval_bicubic_f32_scalar(&rational_curved_f32, &rat_q32);
    for target in PATCH_TARGETS {
        let d = disp.with_forced_backend(Some(target));
        let (_, gottc) = d.eval_bicubic_tensor_f32(&rational_curved_f32, &rat_q32);
        let (_, got32) = d.eval_bicubic_f32(&rational_curved_f32, &rat_q32);
        for i in 0..rat_q32.len() {
            for k in 0..3 {
                assert!((gottc[i].pos[k] - ref_rat32[i].pos[k]).abs() < 1e-4, "rational tensor f32 pos mismatch on {target:?}");
                assert!((gottc[i].norm[k] - ref_rat32[i].norm[k]).abs() < 1e-4, "rational tensor f32 norm mismatch on {target:?}");
                assert!((got32[i].pos[k] - ref_rat32[i].pos[k]).abs() < 1e-4, "rational f32 pos mismatch on {target:?}");
                assert!((got32[i].norm[k] - ref_rat32[i].norm[k]).abs() < 1e-4, "rational f32 norm mismatch on {target:?}");
            }
        }
    }

    // 5. Empty and insufficient control points + warp/tile boundary sizes (1, 3, 4, 15, 16, 17, 31, 32, 33, 63, 64, 65, 129):
    assert!(cpu_eval_bicubic_f32_scalar(&cpts_f32[..15], &corner_queries_f32).is_empty());
    assert!(cpu_eval_bicubic_f32_rayon(&cpts_f32[..15], &corner_queries_f32).is_empty());
    assert!(cpu_eval_bicubic_f32_scalar(&cpts_f32, &[]).is_empty());

    for n in [1usize, 3, 4, 15, 16, 17, 31, 32, 33, 63, 64, 65, 129] {
        let q32: Vec<PatchUvQueryF32> = (0..n)
            .map(|i| PatchUvQueryF32 { u: (i as f32 + 0.5) / (n as f32 + 1.0), v: ((n - i) as f32 + 0.25) / (n as f32 + 1.0), patch_idx: 0, pad: 0 })
            .collect();
        let ref32 = cpu_eval_bicubic_f32_scalar(&cpts_f32, &q32);
        assert_eq!(ref32.len(), n);
        if let Some(dev) = cuda {
            let gen32 = dev.eval_bicubic_tensor_f32(&cpts_f32, &q32, false).expect("cuda gen32");
            for i in 0..n {
                for k in 0..3 {
                    assert!((gen32[i].pos[k] - ref32[i].pos[k]).abs() < 1e-5, "gen32 boundary n={n} i={i} mismatch");
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
            if dev.has_f32_tensor_core() {
                let mma = dev.eval_bicubic_tensor_f32(&cpts_f32, &q32, true).expect("cuda mma");
                for i in 0..n {
                    for k in 0..3 {
                        assert!((mma[i].pos[k] - ref32[i].pos[k]).abs() < 1e-5, "MMA boundary n={n} i={i} mismatch");
                    }
                }
            }
        }
    }
}
