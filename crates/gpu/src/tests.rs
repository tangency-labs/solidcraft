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
