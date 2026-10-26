use super::super::{Executor, JobOptions, Task, open_headless};
use super::*;
use solidcraft_geom::{Mesh, Vec3};
use solidcraft_render::{
    Camera, ProductMesh, ProductScene, StandardView,
    job::{CancelToken, ImageRequest, ImageWork, TaskPoll},
    surface::OpaqueSurface,
};
use std::sync::Arc;

type Guide = [[f64; 4]; 3];
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}
fn diff(a: [f64; 4], b: [f64; 4]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn xyz(a: [f64; 4]) -> [f64; 3] {
    [a[0], a[1], a[2]]
}

// Independent f64 gather oracle: all inputs are averaged linear values, so the
// GPU's accumulated-sample scaling is absent from this numerical implementation.
fn reference(width: usize, input: &[[f64; 4]], guides: &[Guide], step: usize) -> Vec<[f64; 4]> {
    let height = input.len() / width;
    let kernel = [1.0, 4.0, 6.0, 4.0, 1.0];
    input
        .iter()
        .enumerate()
        .map(|(i, &center)| {
            let [a, n, p] = guides[i];
            if a[3] == 0.0 || center[3] < 0.999 {
                return center;
            }
            let mut result = [0.0; 4];
            let mut total = 0.0;
            for (ky, &wy) in kernel.iter().enumerate() {
                for (kx, &wx) in kernel.iter().enumerate() {
                    let x = (i % width) as isize + (kx as isize - 2) * step as isize;
                    let y = (i / width) as isize + (ky as isize - 2) * step as isize;
                    if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                        continue;
                    }
                    let j = y as usize * width + x as usize;
                    let [b, m, q] = guides[j];
                    let value = input[j];
                    if a[3] != b[3] || (value[3] - center[3]).abs() > 0.01 {
                        continue;
                    }
                    let albedo = diff(a, b);
                    let position = diff(q, p);
                    let plane = dot(position, xyz(n)).abs() + dot(position, xyz(m)).abs();
                    let scale = (0.05 * p[3].max(q[3]) * step as f64).max(1e-6);
                    let depth = (n[3] - m[3]).abs() / (8.0 * p[3].max(q[3]) * step as f64).max(1e-6);
                    let color = diff(center, value);
                    let maximum = |v: [f64; 3]| v.into_iter().fold(0.0, f64::max);
                    let color_scale = 0.25 + 0.5 * maximum(xyz(center)).max(maximum(xyz(value)));
                    let weight = wx
                        * wy
                        * (-dot(albedo, albedo) / 0.04 - plane / scale - depth - maximum(color.map(f64::abs)) / color_scale).exp()
                        * dot(xyz(n), xyz(m)).max(0.0).powi(32);
                    for k in 0..3 {
                        result[k] += value[k] * weight;
                    }
                    total += weight;
                }
            }
            for v in result.iter_mut().take(3) {
                *v /= total;
            }
            result[3] = center[3];
            result
        })
        .collect()
}
fn guide(x: usize, y: usize) -> Guide {
    [[0.5, 0.5, 0.5, 1.0], [0.0, 0.0, 1.0, 4.0], [x as f64, y as f64, 0.0, 1.0]]
}
fn fixture(size: usize) -> (Vec<[f64; 4]>, Vec<Guide>) {
    let mut pixels = Vec::new();
    let mut guides = Vec::new();
    for y in 0..size {
        for x in 0..size {
            // Deterministic white noise in [0.4, 0.6]. A pixel-frequency checker would
            // be invisible to the step-2 and step-4 passes, which only reach same-parity pixels.
            let hash = ((x * 7919 + y * 104_729) as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 40;
            let level = 0.4 + 0.2 * hash as f64 / f64::from(1u32 << 24);
            pixels.push([level, level, level, 1.0]);
            guides.push(guide(x, y));
        }
    }
    (pixels, guides)
}
#[test]
fn f64_atrous_reduces_flat_noise_preserves_constant_and_coverage() {
    let (input, guides) = fixture(17);
    let mut result = input.clone();
    for step in [1, 2, 4] {
        result = reference(17, &result, &guides, step);
    }
    let variance = |image: &[[f64; 4]]| {
        let mean = image.iter().map(|p| p[0]).sum::<f64>() / image.len() as f64;
        image.iter().map(|p| (p[0] - mean).powi(2)).sum::<f64>() / image.len() as f64
    };
    println!("noise variance {} -> {}", variance(&input), variance(&result));
    assert!(variance(&result) < variance(&input) / 4.0, "noise variance {} -> {}", variance(&input), variance(&result));
    assert_eq!(reference(17, &vec![[0.7, 0.3, 0.1, 0.5]; 289], &guides, 1), vec![[0.7, 0.3, 0.1, 0.5]; 289]);
    let constant = reference(17, &vec![[0.75, 0.75, 0.75, 1.0]; 289], &guides, 4);
    assert!(constant.iter().all(|p| p[..3].iter().all(|x| (*x - 0.75).abs() < 1e-14)));
}
#[test]
fn f64_atrous_preserves_body_normal_depth_and_albedo_edges() {
    for edge in 0..5 {
        let mut pixels = Vec::new();
        let mut guides = Vec::new();
        for y in 0..17 {
            for x in 0..17 {
                let mut g = guide(x, y);
                let c = if x < 8 { 0.0 } else { 1.0 };
                if x >= 8 {
                    match edge {
                        0 => g[0][3] = 2.0,
                        1 => g[1] = [1.0, 0.0, 0.0, 4.0],
                        2 => g[2][2] = 10.0,
                        4 => g[1][3] = 1004.0,
                        _ => g[0] = [1.0, 1.0, 1.0, 1.0],
                    }
                }
                if edge == 3 && x < 8 {
                    g[0] = [0.0, 0.0, 0.0, 1.0];
                }
                pixels.push([c, c, c, 1.0]);
                guides.push(g);
            }
        }
        let mut filtered = pixels.clone();
        for step in [1, 2, 4] {
            filtered = reference(17, &filtered, &guides, step);
        }
        assert!(filtered.iter().zip(&pixels).all(|(a, b)| (a[0] - b[0]).abs() < 1e-12), "edge {edge}");
    }
}
fn context() -> Option<Arc<Context>> {
    let (_, device, queue) = match open_headless() {
        Ok(v) => v,
        Err(e) => {
            // The path tracer runs on Vulkan or Metal only: where the backends in use leave both out
            // (`WGPU_BACKEND=dx12`), it is unavailable by design, not missing.
            let available = !crate::select::path_backends().is_empty();
            assert!(!available || std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none(), "required GPU unavailable: {e}");
            return None;
        }
    };
    Some(Executor::new(device, queue).unwrap().context)
}
fn request(size: u32) -> ImageRequest {
    let mut camera = Camera { distance: 4.0, ..Default::default() };
    camera.set_view(StandardView::Top);
    ImageRequest {
        integrator: Default::default(),
        raytrace: None,
        camera,
        width: size,
        height: size,
        background: [0; 3],
        transparent_background: false,
        environment: None,
        textures: None,
    }
}
fn plane() -> ProductScene {
    let mesh = Mesh {
        positions: vec![Vec3::new(-1.0, -1.0, 0.0), Vec3::new(1.0, -1.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(-1.0, 1.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    };
    ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![ProductMesh {
            name: "original denoise plane".into(),
            mesh: Arc::new(mesh),
            color: [255; 3],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(0.5, 0.5, 0.5), 0.0, 1.0, 1.5).unwrap()),
            transmission: None,
            faces: Vec::new(),
        }],
    }
}
fn read_guides(context: &Context, buffer: &wgpu::Buffer) -> Vec<[f32; 4]> {
    let staging = context.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = context.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    let submission = context.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |v| {
        tx.send(v).unwrap();
    });
    context.device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(TIMEOUT) }).unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = staging.slice(..).get_mapped_range().unwrap();
    let values = mapped.as_chunks::<16>().0.iter().map(|v| [0, 4, 8, 12].map(|i| f32::from_le_bytes(v[i..i + 4].try_into().unwrap()))).collect();
    drop(mapped);
    staging.unmap();
    values
}
#[test]
fn denoise_admission_counts_guides_and_ping_pong_only_when_enabled() {
    let scene = plane();
    let request = request(32);
    let plain = super::super::admission(&scene, &request, JobOptions::default()).unwrap();
    let filtered = super::super::admission(&scene, &request, JobOptions { denoise: true, ..Default::default() }).unwrap();
    assert_eq!(filtered.pixel_scratch_bytes - plain.pixel_scratch_bytes, 32 * 32 * 80);
    assert_eq!(filtered.estimated_total_bytes - plain.estimated_total_bytes, 32 * 32 * 80);
    let large = ImageRequest { width: 2000, height: 2000, ..request.clone() };
    assert!(super::super::admission(&scene, &large, JobOptions::default()).is_ok());
    // The production envelope deliberately admits this former 512 MiB refusal.
    // Still charge the guides/ping-pong in full, and reject outside the new edge.
    let large_plain = super::super::admission(&scene, &large, JobOptions::default()).unwrap();
    let large_filtered = super::super::admission(&scene, &large, JobOptions { denoise: true, ..Default::default() }).unwrap();
    assert_eq!(large_filtered.pixel_scratch_bytes - large_plain.pixel_scratch_bytes, 2000 * 2000 * 80);
    assert!(large_filtered.estimated_total_bytes > 512 * 1024 * 1024);
    assert!(large_filtered.estimated_total_bytes <= solidcraft_render::job::GPU_PATH_LIMITS.max_scratch_bytes);
    let excessive = ImageRequest { width: (solidcraft_render::job::GPU_PATH_LIMITS.max_dimension + 1) as u32, ..request.clone() };
    for denoise in [false, true] {
        assert!(super::super::admission(&scene, &excessive, JobOptions { denoise, ..Default::default() }).is_err());
    }
    assert!(!serde_json::from_str::<JobOptions>("{}").unwrap().denoise);
    assert!(serde_json::from_str::<JobOptions>(r#"{"denoise":"true"}"#).is_err());
}
#[test]
fn gpu_atrous_matches_f64_guides_and_leaves_input_unchanged() {
    let Some(context) = context() else {
        return;
    };
    let (mut input, mut guides) = fixture(13);
    for y in 0..13 {
        for x in 7..13 {
            guides[y * 13 + x][0][3] = 2.0;
            input[y * 13 + x][0] += 0.8;
        }
    }
    guides[0][0][3] = 0.0;
    input[1][3] = 0.5;
    let bytes: Vec<u8> = input.iter().flatten().flat_map(|x| (*x as f32).to_le_bytes()).collect();
    let device = &context.device;
    let make = |label, bytes: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        })
    };
    let raw = make("denoise fixture raw", &bytes);
    let dummy = make("denoise unused geometry", &[0; 64]);
    let mut task = Task::new(context.clone(), plane(), request(13), JobOptions::default(), CancelToken::default()).unwrap();
    task.sample = 1;
    let filter = Resources::new(&context, &task.uniform, [&dummy, &dummy, &dummy, &dummy, &dummy], &raw, bytes.len() as u64).unwrap();
    let guide_bytes: Vec<u8> = guides.iter().flatten().flatten().flat_map(|x| (*x as f32).to_le_bytes()).collect();
    context.queue.write_buffer(&filter._guides, 0, &guide_bytes);
    let before = task.read_linear_buffer(&raw).unwrap();
    filter.run_filter(&task).unwrap();
    assert_eq!(before, task.read_linear_buffer(&raw).unwrap());
    let actual = task.read_linear_buffer(filter.output()).unwrap();
    let mut expected = input;
    for step in [1, 2, 4] {
        expected = reference(13, &expected, &guides, step);
    }
    for (a, b) in actual.iter().zip(&expected) {
        for (x, y) in a.iter().zip(b) {
            assert!((f64::from(*x) - *y).abs() < 2e-5, "{a:?} vs {b:?}");
        }
    }
}
#[test]
fn gpu_denoise_default_false_is_identical_and_true_preserves_raw() {
    let Some(context) = context() else {
        return;
    };
    let mut outputs = Vec::new();
    let mut pngs = Vec::new();
    for options in [
        JobOptions { samples: 4, ..Default::default() },
        JobOptions { samples: 4, denoise: false, ..Default::default() },
        JobOptions { samples: 4, denoise: true, ..Default::default() },
    ] {
        let mut task = Task::new(context.clone(), plane(), request(8), options, CancelToken::default()).unwrap();
        assert_eq!(task.denoiser.is_some(), options.denoise);
        while task.sample < 4 {
            task.complete_sample().unwrap();
        }
        let raw = task.read_linear().unwrap();
        if let Some(filter) = &task.denoiser {
            filter.run(&task).unwrap();
            assert_eq!(raw, task.read_linear().unwrap());
            let filtered = task.read_linear_buffer(filter.output()).unwrap();
            assert!(filtered.iter().all(|p| p.iter().all(|v| v.is_finite())));
            let guides = read_guides(&context, &filter._guides);
            assert_eq!(guides[0], [0.0; 4]);
            let center = (4 * 8 + 4) * 3;
            assert_eq!(guides[center], [0.5, 0.5, 0.5, 1.0]);
            assert_eq!(guides[center + 1], [0.0, 0.0, 1.0, 16.0]);
            assert!((guides[center + 2][0] - 0.2).abs() < 1e-6);
            assert!((guides[center + 2][1] + 0.2).abs() < 1e-6);
            assert!((guides[center + 2][3] - 0.4).abs() < 1e-6);
        }
        outputs.push(raw);
        for _ in 0..100 {
            if let TaskPoll::Complete(png) = task.step(4096).unwrap() {
                pngs.push(png);
                break;
            }
        }
    }
    assert_eq!(outputs[0], outputs[1]);
    assert_eq!(outputs[0], outputs[2]);
    assert_eq!(pngs.len(), 3);
    assert_eq!(pngs[0], pngs[1]);
}
#[test]
fn gpu_denoise_preflight_reports_limits_without_changing_document() {
    let Some(context) = context() else {
        return;
    };
    let mut session = solidcraft_engine::Session::default();
    session.set_render_job_host(Some(solidcraft_render::job::CpuHost::cooperative())).unwrap();
    session.register_render_job_host(solidcraft_render::job::ImageHost::with_executor(Arc::new(Executor { context })).unwrap()).unwrap();
    session.execute("solid.box", &serde_json::json!({"length":2,"width":2,"height":1})).unwrap();
    let before = session.doc.to_json();
    let mut params = serde_json::json!({"progressive":true,"denoise":true,"settings":{"width":8,"height":8}});
    let reply = session.execute("render.preflight", &params).unwrap();
    assert_eq!(reply["options"]["denoise"], true);
    assert_eq!(reply["denoising"]["passes"], 3);
    assert_eq!(reply["denoising"]["raw_linear_accumulator_preserved"], true);
    assert_eq!(before, session.doc.to_json());
    params["progressive"] = serde_json::json!(false);
    assert!(session.execute("render.preflight", &params).is_err());
    params["denoise"] = serde_json::json!(false);
    let reply = session.execute("render.preflight", &params).unwrap();
    assert!(reply.get("denoising").is_none());
}
