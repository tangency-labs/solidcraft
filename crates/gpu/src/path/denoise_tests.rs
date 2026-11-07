use super::super::{Executor, JobOptions, Task, open_headless};
use super::*;
use solidcore_record::Vec3;
use solidcraft_geom::Mesh;
use solidcraft_render::{
    Camera, ProductMesh, ProductScene, StandardView,
    job::{CancelToken, ImageRequest, ImageWork, TaskPoll},
    surface::OpaqueSurface,
};
use std::sync::Arc;

type Guide = [[f64; 4]; 6];
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
            let [a, n, p, ar, nr, pr] = guides[i];
            let optical = a[3] < 0.0;
            if a[3] == 0.0 || center[3] < 0.999 || (optical && step > 2) {
                return center;
            }
            let mut result = [0.0; 4];
            let mut total = 0.0;
            let maximum = |v: [f64; 3]| v.into_iter().fold(0.0, f64::max);
            let center_peak = maximum(xyz(center));
            let mut support = Vec::new();
            for (ky, &wy) in kernel.iter().enumerate() {
                for (kx, &wx) in kernel.iter().enumerate() {
                    let x = (i % width) as isize + (kx as isize - 2) * step as isize;
                    let y = (i / width) as isize + (ky as isize - 2) * step as isize;
                    if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                        continue;
                    }
                    let j = y as usize * width + x as usize;
                    let [b, m, q, br, mr, qr] = guides[j];
                    let value = input[j];
                    if a[3] != b[3] || (value[3] - center[3]).abs() > 0.01 {
                        continue;
                    }
                    let albedo = diff(a, b);
                    let position = diff(q, p);
                    let plane = dot(position, xyz(n)).abs() + dot(position, xyz(m)).abs();
                    let scale = (0.05 * p[3].max(q[3]) * step as f64).max(1e-6);
                    let depth = (n[3] - m[3]).abs() / (8.0 * p[3].max(q[3]) * step as f64).max(1e-6);
                    let mut guide_weight = (-dot(albedo, albedo) / 0.04 - plane / scale - depth).exp() * dot(xyz(n), xyz(m)).max(0.0).powi(32);
                    if ar[3] != br[3] {
                        continue;
                    }
                    if ar[3] != 0.0 {
                        let da = diff(ar, br);
                        let dp = diff(qr, pr);
                        let footprint = pr[3].max(qr[3]) * step as f64;
                        let plane = dot(dp, xyz(nr)).abs() + dot(dp, xyz(mr)).abs();
                        let depth = (nr[3] - mr[3]).abs() / (8.0 * footprint).max(1e-6);
                        guide_weight *=
                            (-dot(da, da) / 0.04 - plane / (0.05 * footprint).max(1e-6) - depth).exp() * dot(xyz(nr), xyz(mr)).max(0.0).powi(32);
                    }
                    let tap_peak = maximum(xyz(value));
                    if !optical && step == 1 && (1..=3).contains(&kx) && (1..=3).contains(&ky) && i != j && guide_weight >= 0.5 {
                        support.push(tap_peak);
                    }
                    let color = maximum(diff(center, value).map(f64::abs));
                    let high = center_peak.max(tap_peak);
                    let bound = 16.0 * (0.25 + center_peak.min(tap_peak));
                    let contrast = if high <= bound { 1.0 } else { (bound / high).powi(2) };
                    let color_weight = if optical { 4.0 } else { 1.0 };
                    let weight = wx * wy * guide_weight * (-color_weight * color / (0.25 + 0.5 * high)).exp() * contrast;
                    for k in 0..3 {
                        result[k] += value[k] * weight;
                    }
                    total += weight;
                }
            }
            for v in result.iter_mut().take(3) {
                *v /= total;
            }
            if support.len() >= 5 {
                let support_peak = support.into_iter().fold(0.0, f64::max);
                let cap = 2.0 * support_peak + 0.25;
                let peak = maximum(xyz(result));
                if center_peak > 16.0 * (0.25 + support_peak) && peak > cap {
                    for v in &mut result[..3] {
                        *v *= cap / peak;
                    }
                }
            }
            result[3] = center[3];
            result
        })
        .collect()
}
fn guide(x: usize, y: usize) -> Guide {
    [[0.5, 0.5, 0.5, 1.0], [0.0, 0.0, 1.0, 4.0], [x as f64, y as f64, 0.0, 1.0], [0.0; 4], [0.0; 4], [0.0; 4]]
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
        studio_weights: None,
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
    assert_eq!(filtered.pixel_scratch_bytes - plain.pixel_scratch_bytes, 32 * 32 * 128);
    assert_eq!(filtered.estimated_total_bytes - plain.estimated_total_bytes, 32 * 32 * 128);
    let large = ImageRequest { width: 2000, height: 2000, ..request.clone() };
    assert!(super::super::admission(&scene, &large, JobOptions::default()).is_ok());
    // The production envelope deliberately admits this former 512 MiB refusal.
    // Still charge the guides/ping-pong in full, and reject outside the new edge.
    let large_plain = super::super::admission(&scene, &large, JobOptions::default()).unwrap();
    let large_filtered = super::super::admission(&scene, &large, JobOptions { denoise: true, ..Default::default() }).unwrap();
    assert_eq!(large_filtered.pixel_scratch_bytes - large_plain.pixel_scratch_bytes, 2000 * 2000 * 128);
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
fn gpu_atrous_matches_f64_with_firefly_line_and_material_boundary() {
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
    for y in 0..13 {
        input[y * 13 + 10] = [256.0, 64.0, 16.0, 1.0];
    }
    let isolated = 6 * 13 + 3;
    let material_boundary = 2 * 13 + 6;
    input[isolated] = [512.0, 128.0, 32.0, 1.0];
    input[material_boundary] = input[isolated];
    // Same body but incompatible albedo. The neighbouring floor cannot
    // supply support for capping a highlight on this material patch.
    guides[material_boundary][0] = [1.0; 4];
    let optical_glint = 2 * 13 + 8;
    for y in 1..=3 {
        for x in 7..=9 {
            guides[y * 13 + x][0][3] = f64::from(f32::from_bits(0xa004_0400));
        }
    }
    input[optical_glint] = [1024.0, 256.0, 64.0, 1.0];
    guides[0][0][3] = 0.0;
    input[1][3] = 0.5;
    let samples = 8;
    let bytes: Vec<u8> = input.iter().flatten().flat_map(|x| (*x as f32 * samples as f32).to_le_bytes()).collect();
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
    task.sample = samples;
    let filter = Resources::new(&context, &task.uniform, [&dummy, &dummy, &dummy, &dummy, &dummy], &raw, bytes.len() as u64, false).unwrap();
    let guide_bytes: Vec<u8> = guides.iter().flatten().flatten().flat_map(|x| (*x as f32).to_le_bytes()).collect();
    context.queue.write_buffer(&filter._guides, 0, &guide_bytes);
    let before = task.read_linear_buffer(&raw).unwrap();
    filter.run_filter(&task, false).unwrap();
    assert_eq!(before, task.read_linear_buffer(&raw).unwrap());
    let actual = task.read_linear_buffer(filter.output()).unwrap();
    let first_pass = reference(13, &input, &guides, 1);
    assert!(first_pass[isolated][0] < 1.5);
    let expected = reference(13, &reference(13, &first_pass, &guides, 2), &guides, 4);
    for (a, b) in actual.iter().zip(&expected) {
        assert_eq!(a[3].to_bits(), (b[3] as f32).to_bits());
        for (x, y) in a[..3].iter().zip(&b[..3]) {
            assert!((f64::from(*x) - *y).abs() < 2e-5 * y.abs().max(1.0), "{a:?} vs {b:?}");
        }
    }
    assert!(actual[isolated][0] < 0.7, "the isolated firefly must come down to its neighbourhood");
    assert!(actual[6 * 13 + 10][0] > 254.0, "a supported bright line must survive");
    assert!(actual[material_boundary][0] > 511.0, "incompatible guide support must preserve the material highlight");
    assert!(actual[optical_glint][0] > 1023.0, "a single optical glint with matching guides must survive");
}
#[test]
fn gpu_optical_filter_preserves_edges_on_the_same_reflected_body() {
    let Some(context) = context() else {
        return;
    };
    let device = &context.device;
    let input: Vec<[f32; 4]> = (0..25).map(|i| if i == 12 { [0.8, 0.8, 0.8, 1.0] } else { [0.4, 0.4, 0.4, 1.0] }).collect();
    let bytes: Vec<_> = input.iter().flatten().flat_map(|x| x.to_le_bytes()).collect();
    let raw = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mixed glass edge radiance"),
        contents: &bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let dummy = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("unused optical edge geometry"),
        contents: &[0; 64],
        usage: wgpu::BufferUsages::STORAGE,
    });
    let mut task = Task::new(context.clone(), plane(), request(5), JobOptions::default(), CancelToken::default()).unwrap();
    task.sample = 1;
    let filter = Resources::new(&context, &task.uniform, [&dummy, &dummy, &dummy, &dummy, &dummy], &raw, bytes.len() as u64, false).unwrap();
    for edge in 0..4 {
        let mut g = guide(0, 0);
        g[0][3] = f64::from(f32::from_bits(0xa004_0403));
        g[3] = [0.5, 0.5, 0.5, 2.0];
        g[4] = [0.0, 0.0, 1.0, 4.0];
        g[5] = [0.0, 0.0, 0.0, 1.0];
        let mut guides = vec![g; 25];
        match edge {
            0 => guides[12][3] = [1.0, 1.0, 1.0, 2.0],
            1 => guides[12][4] = [1.0, 0.0, 0.0, 4.0],
            2 => guides[12][4][3] = 1000.0,
            _ => guides[12][5][2] = 10.0,
        }
        let packed: Vec<_> = guides.iter().flatten().flatten().flat_map(|x| (*x as f32).to_le_bytes()).collect();
        context.queue.write_buffer(&filter._guides, 0, &packed);
        filter.run_filter(&task, false).unwrap();
        let actual = task.read_linear_buffer(filter.output()).unwrap();
        assert!(actual[12][0] > 0.799, "reflected edge {edge}: {:?}", actual[12]);
        assert_eq!(task.read_linear_buffer(&raw).unwrap(), input);
    }
}
fn optical_plane(z: f64, albedo: Vec3) -> ProductMesh {
    let mut body = plane().meshes.remove(0);
    for point in &mut Arc::make_mut(&mut body.mesh).positions {
        point.x *= 3.0;
        point.y *= 3.0;
        point.z = z;
    }
    body.surface = Some(OpaqueSurface::new(albedo, 0.0, 0.7, 1.0).unwrap());
    body
}
fn optical_box() -> Mesh {
    let faces = [
        ([[-3., -3., 0.5], [3., -3., 0.5], [3., 3., 0.5], [-3., 3., 0.5]], Vec3::Z),
        ([[-3., 3., -0.5], [3., 3., -0.5], [3., -3., -0.5], [-3., -3., -0.5]], -Vec3::Z),
        ([[3., -3., -0.5], [3., 3., -0.5], [3., 3., 0.5], [3., -3., 0.5]], Vec3::X),
        ([[-3., 3., -0.5], [-3., -3., -0.5], [-3., -3., 0.5], [-3., 3., 0.5]], -Vec3::X),
        ([[3., 3., -0.5], [-3., 3., -0.5], [-3., 3., 0.5], [3., 3., 0.5]], Vec3::Y),
        ([[-3., -3., -0.5], [3., -3., -0.5], [3., -3., 0.5], [-3., -3., 0.5]], -Vec3::Y),
    ];
    let mut mesh = Mesh::default();
    for (points, normal) in faces {
        let start = mesh.positions.len() as u32;
        mesh.positions.extend(points.map(|[x, y, z]| Vec3::new(x, y, z)));
        mesh.normals.extend([normal; 4]);
        mesh.triangles.extend([[start, start + 1, start + 2], [start, start + 2, start + 3]]);
        mesh.tri_face.extend([0; 2]);
    }
    mesh
}

#[test]
fn gpu_denoise_optical_guides_track_branches_and_certified_snell_endpoints() {
    use solidcraft_render::{
        glass_model::{GlassModel, ThinFilm},
        job::{AreaLight, AreaLights, StudioLighting, ThinLens},
        transmission::Transmission,
    };
    let Some(context) = context() else { return };
    let mut image = request(1);
    let incident = 0.4_f64;
    image.camera.pitch = std::f64::consts::FRAC_PI_2 - incident;
    image.camera.yaw = 0.0;
    image.camera.fov = 0.4;
    let reflected = optical_plane(6.0, Vec3::new(0.2, 0.4, 0.6));
    let transmitted = optical_plane(-3.0, Vec3::new(0.7, 0.3, 0.2));
    let base = optical_plane(0.0, Vec3::ZERO);
    let options = JobOptions { samples: 1, max_depth: 4, denoise: true, ..Default::default() };
    let probe = |meshes: Vec<ProductMesh>, options| {
        let bounds = meshes.iter().fold(solidcraft_geom::Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
        let task = Task::new(context.clone(), ProductScene { meshes, bounds }, image.clone(), options, CancelToken::default()).unwrap();
        let filter = task.denoiser.as_ref().unwrap();
        filter.generate_guides(&task).unwrap();
        assert!(read_guides(&context, &task.failures).iter().all(|word| *word == [0.0; 4]), "guide tracing cannot write production failures");
        read_guides(&context, &filter._guides)
    };
    let tag = |reflection: u32, transmission: u32| 0xa0000000_u32 | (1_u32 << 18) | (reflection << 9) | transmission;
    let empty = vec![[0.0; 4]; 6];
    let mut mirror = base.clone();
    mirror.surface = Some(OpaqueSurface::new(Vec3::new(1.0, 1.0, 1.0), 1.0, 0.045, 1.5).unwrap());
    let guide = probe(vec![mirror.clone(), reflected.clone()], options);
    assert_eq!(guide[0], [0.2, 0.4, 0.6, f32::from_bits(tag(2, 0))]);
    assert_eq!(&guide[1][..3], &[0.0, 0.0, -1.0]);
    assert!(guide[1][3] < 0.0, "the reflected endpoint is behind the camera");
    assert!((guide[2][1] - (6.0 * incident.tan()) as f32).abs() < 2e-5);
    assert!(guide[2][3] >= 1.6, "the endpoint behind the camera retains the primary footprint");
    let lens = ThinLens { aperture_radius_mm: 0.1, focus_distance_mm: 4.0 };
    assert_eq!(probe(vec![mirror, reflected.clone()], JobOptions { lens: Some(lens), ..options }), empty);

    let mut thin = base.clone();
    thin.surface = Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.7, 1.5).unwrap());
    thin.transmission = Some(Transmission::new(Vec3::ZERO, Some(0.0)).unwrap());
    let thin_scene = vec![thin.clone(), reflected.clone(), transmitted.clone()];
    let guide = probe(thin_scene.clone(), options);
    assert_eq!(guide[0], [0.7, 0.3, 0.2, f32::from_bits(tag(2, 3))]);
    assert_eq!(guide[3], [0.2, 0.4, 0.6, 2.0]);
    assert_eq!(&guide[4][..3], &[0.0, 0.0, -1.0]);
    assert_eq!(&guide[1][..3], &[0.0, 0.0, 1.0]);
    assert!((guide[2][1] - (3.0 * incident.tan()) as f32).abs() < 2e-5);
    assert_eq!(probe(vec![thin.clone(), transmitted.clone()], options), empty, "both weighted branches need an opaque endpoint");
    let emitter = AreaLight { center: [0.0, 2.0, 5.0], u: [2.0, 0.0, 0.0], v: [0.0, 2.0, 0.0], radiance: [1.0; 3] };
    let blocked = StudioLighting { area_lights: AreaLights::new(&[emitter]).unwrap(), ..Default::default() };
    assert_eq!(probe(thin_scene, JobOptions { lighting: Some(blocked), ..options }), empty);
    let film = GlassModel { roughness: 0.0, dispersion: None, thin_film: Some(ThinFilm { ior: 1.35, thickness_nm: 200.0 }) };
    let mut modeled = thin.clone();
    modeled.transmission = Some(modeled.transmission.unwrap().with_model(film).unwrap());
    assert_eq!(probe(vec![modeled, reflected.clone(), transmitted.clone()], options), empty);
    let mut matched = thin.clone();
    matched.surface = base.surface;
    let guide = probe(vec![matched, reflected.clone(), transmitted.clone()], options);
    assert_eq!(guide[0][3].to_bits(), tag(0, 3));

    let mut closed = thin;
    closed.mesh = Arc::new(optical_box());
    closed.transmission = Some(Transmission::new(Vec3::ZERO, None).unwrap());
    let closed_scene = vec![closed, reflected, transmitted];
    let guide = probe(closed_scene.clone(), JobOptions { nested_media: true, ..options });
    assert_eq!(guide[0], [0.7, 0.3, 0.2, f32::from_bits(tag(2, 3))]);
    assert_eq!(guide[3], [0.2, 0.4, 0.6, 2.0]);
    assert_eq!(&guide[4][..3], &[0.0, 0.0, -1.0]);
    let internal = (incident.sin() / 1.5).asin();
    let snell_y = 2.0 * incident.tan() + internal.tan();
    assert!((f64::from(guide[2][1]) - snell_y).abs() < 2e-5, "Snell displacement {guide:?} vs {snell_y}");
    assert_eq!(probe(closed_scene, JobOptions { max_depth: 2, ..options }), empty);
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
            use std::sync::atomic::Ordering;
            assert!(!filter.guides_ready.load(Ordering::Relaxed));
            filter.run(&task).unwrap();
            assert!(filter.guides_ready.load(Ordering::Relaxed));
            assert_eq!(raw, task.read_linear().unwrap());
            let filtered = task.read_linear_buffer(filter.output()).unwrap();
            assert!(filtered.iter().all(|p| p.iter().all(|v| v.is_finite())));
            let guides = read_guides(&context, &filter._guides);
            assert_eq!(guides[0], [0.0; 4]);
            let center = (4 * 8 + 4) * 6;
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
