//! The viewport's wood grain (the WGSL `wood_sample`) against the CPU's (`wood::sample`), at the
//! same points: the shader and its CPU reference show one pattern.

use solidcore_record::Vec3;
use solidcraft_viewport::wood::{self, Frame, WoodShade};

const WGSL: &str = include_str!("../shaders/wood.wgsl");
use wgpu::util::DeviceExt;

const PROBE: &str = r#"
@group(0) @binding(0) var<storage, read> rec: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> pts: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> out: array<vec4<f32>>;
fn wood_texel(slot: u32, k: u32) -> vec4<f32> { return rec[k]; }
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (3u * i >= arrayLength(&pts)) { return; }
    out[i] = wood_sample(1u, wood_grain(1u, pts[3u * i].xyz), wood_step(1u, pts[3u * i + 1u].xyz), wood_step(1u, pts[3u * i + 2u].xyz));
}
"#;

fn board_shade(species: &str) -> WoodShade {
    // A 600 × 100 × 20 mm board along X.
    let (x, y, z) = (600.0, 100.0, 20.0);
    let p: Vec<Vec3> =
        (0..8).map(|i| Vec3::new(if i & 1 == 0 { 0.0 } else { x }, if i & 2 == 0 { 0.0 } else { y }, if i & 4 == 0 { 0.0 } else { z })).collect();
    let t = vec![[0, 2, 1], [1, 2, 3], [4, 5, 6], [5, 7, 6], [0, 1, 4], [1, 5, 4], [2, 6, 3], [3, 6, 7], [0, 4, 2], [2, 4, 6], [1, 3, 5], [3, 7, 5]];
    let mut look = wood::species_look(species).unwrap();
    look.seed = 0x51_7cc1;
    WoodShade { look, to_grain: wood::frame_for_mesh(&p, &t, Frame::default()).unwrap() }
}

fn gpu_samples(device: &wgpu::Device, queue: &wgpu::Queue, shade: &WoodShade, points: &[[[f32; 4]; 3]]) -> Vec<[f32; 4]> {
    let source = format!("{WGSL}\n{PROBE}");
    let module =
        device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("wood_probe"), source: wgpu::ShaderSource::Wgsl(source.into()) });
    let record: Vec<f32> = shade.texels().iter().flatten().copied().collect();
    let flat: Vec<f32> = points.iter().flatten().flatten().copied().collect();
    let bytes = |v: &[f32]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    let storage = |label, data: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: data, usage: wgpu::BufferUsages::STORAGE })
    };
    let rec = storage("rec", &bytes(&record));
    let pts = storage("pts", &bytes(&flat));
    let size = (points.len() * 16) as u64;
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("out"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("read"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("wood_probe"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("wood_probe"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: rec.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: pts.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: out.as_entire_binding() },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups((points.len() as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out, 0, &read, 0, size);
    queue.submit([encoder.finish()]);
    read.slice(..).map_async(wgpu::MapMode::Read, |r| r.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let data = read.slice(..).get_mapped_range().unwrap();
    data.as_chunks::<16>()
        .0
        .iter()
        .map(|c| std::array::from_fn(|k| f32::from_le_bytes([c[4 * k], c[4 * k + 1], c[4 * k + 2], c[4 * k + 3]])))
        .collect()
}

/// Every species on the GPU matches the CPU: colours within 2/255 and roughness within 0.01 at
/// all but a handful of points (where a point lies a rounding away from a ring's edge and the two
/// fall on either side of it).
#[test]
fn the_gpu_wood_matches_the_cpu_wood() {
    let Ok((adapter, device, queue)) = crate::open_headless(crate::test_env::policy()) else {
        crate::test_env::skip_unless_required(crate::test_env::required(), &"no GPU adapter");
        return;
    };
    eprintln!("GPU: {}", adapter.get_info().name);
    // Points on the board's faces and inside it.
    // Points on the board's faces and inside it, each with a pixel's two steps: none, small,
    // and long ones along and across the grain (a far and a steep view).
    let points: Vec<[[f32; 4]; 3]> = (0..4096)
        .map(|i| {
            let f = |k: u32, n: f32| ((i as u32).wrapping_mul(k) % 10007) as f32 / 10007.0 * n;
            let a = (i % 5) as f32 * 0.3;
            let b = (i % 7) as f32 * 0.9;
            [[f(7919, 600.0), f(104_729, 100.0), if i % 3 == 0 { 20.0 } else { f(1_299_709, 20.0) }, 0.0], [b, a, 0.0, 0.0], [0.1 * a, 0.0, a, 0.0]]
        })
        .collect();
    for species in wood::SPECIES {
        let mut shade = board_shade(species.id);
        // Half the species as glued-up stock: boards 30 mm wide and 250 mm long.
        if species.id.len() % 2 == 0 {
            shade.look.stock = [30.0, 250.0, 4.0];
        }
        // And some with knots, some as burl, some with even rings.
        shade.look.knots = [0.0, 1.0, 0.6][species.id.len() % 3];
        shade.look.burl = [0.0, 0.0, 1.0, 0.5][species.id.len() % 4];
        shade.look.variation = [1.0, 2.0, 0.0, 1.0, 0.5][species.id.len() % 5];
        let gpu = gpu_samples(&device, &queue, &shade, &points);
        let mut off = 0;
        let mut worst = 0f32;
        for (p, g) in points.iter().zip(&gpu) {
            let v = |q: [f32; 4]| Vec3::new(q[0] as f64, q[1] as f64, q[2] as f64);
            let c = shade.sample_seen(v(p[0]), v(p[1]), v(p[2]));
            let d = (0..3).map(|k| (c.srgb[k] - g[k]).abs()).fold((c.roughness - g[3]).abs(), f32::max);
            if d > 2.0 / 255.0 {
                off += 1;
            } else {
                worst = worst.max(d);
            }
        }
        eprintln!("{}: {off} of {} points off, worst of the rest {:.5}", species.id, points.len(), worst);
        assert!(off * 200 <= points.len(), "{}: {off} of {} points differ by more than 2/255", species.id, points.len());
    }
}
