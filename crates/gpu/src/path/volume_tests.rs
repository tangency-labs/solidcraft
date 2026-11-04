use super::*;
use solidcraft_render::job::{StudioLighting, volume::HomogeneousVolume};

fn empty() -> ProductScene {
    ProductScene { meshes: Vec::new(), bounds: Aabb3::EMPTY }
}
fn slab() -> HomogeneousVolume {
    HomogeneousVolume {
        min_mm: [-10000.0, -10000.0, -1.0],
        max_mm: [10000.0, 10000.0, 1.0],
        extinction_per_mm: 0.5,
        scattering_albedo: [0.8; 3],
        anisotropy: 0.0,
    }
}
fn settings(volume: HomogeneousVolume) -> JobOptions {
    JobOptions {
        volume: Some(volume),
        lighting: Some(StudioLighting {
            punctual_lights: Default::default(),
            directional_lights: Default::default(),
            environment: [1.0; 3],
            sun: None,
            area_lights: Default::default(),
        }),
        samples: 1024,
        max_depth: 1,
        seed: 3951,
        ..Default::default()
    }
}
fn camera() -> ImageRequest {
    let mut r = request(8);
    r.camera = solidcraft_render::Camera { distance: 4.0, pitch: std::f64::consts::FRAC_PI_2, yaw: 0.0, fov: 0.001, ..Default::default() };
    r
}

#[test]
fn volume_admission_refuses_glass_surface_denoising_legacy_lighting_and_lost_position_precision() {
    let options = settings(slab());
    assert!(admission(&empty(), &camera(), options).is_ok());
    assert!(admission(&glass_slab(true), &camera(), options).unwrap_err().message.contains("glass"));
    assert!(admission(&empty(), &camera(), JobOptions { denoise: true, ..options }).unwrap_err().message.contains("denois"));
    assert!(admission(&empty(), &camera(), JobOptions { lighting: None, ..options }).is_err());
    let too_dense = HomogeneousVolume { min_mm: [1e8; 3], max_mm: [1e8 + 100.0; 3], extinction_per_mm: 1000.0, ..slab() };
    assert!(admission(&empty(), &camera(), JobOptions { volume: Some(too_dense), ..options }).is_err());
    let base = JobOptions { volume: None, ..options };
    let a = admission(&empty(), &camera(), base).unwrap();
    let b = admission(&empty(), &camera(), options).unwrap();
    assert_eq!(a.estimated_total_bytes, b.estimated_total_bytes, "homogeneous metadata fits the existing task allowance, no unbilled voxel buffers");
}

#[test]
fn gpu_volume_beer_coverage_and_single_scatter_match_independent_slab_quadrature() {
    let Some(executor) = executor() else { return };
    let medium = slab();
    // Independent integration over collision depth and the isotropic outgoing
    // cosine. The finite 20-m lateral bounds are optically opaque at grazing
    // angles, making the infinite-slab formula indistinguishable at this tolerance.
    let mut expected = 0.0;
    for z in 0..512 {
        let depth = (f64::from(z) + 0.5) * 2.0 / 512.0;
        let mut angular = 0.0;
        for i in 0..1024 {
            let cosine = -1.0 + (f64::from(i) + 0.5) * 2.0 / 1024.0;
            let exit = if cosine > 0.0 { depth / cosine } else { (2.0 - depth) / (-cosine) };
            angular += (-medium.extinction_per_mm * exit).exp() / 1024.0;
        }
        expected += medium.extinction_per_mm * (-medium.extinction_per_mm * depth).exp() * 0.8 * angular * 2.0 / 512.0;
    }
    for scattering in [false, true] {
        let v = if scattering { medium } else { HomogeneousVolume { scattering_albedo: [0.0; 3], ..medium } };
        let options = settings(v);
        let mut task = Task::new(executor.context.clone(), empty(), camera(), options, CancelToken::default()).unwrap();
        while task.sample < options.samples {
            task.dispatch().unwrap();
        }
        let pixels = task.read_linear().unwrap();
        let alpha = pixels.iter().map(|p| f64::from(p[3])).sum::<f64>() / pixels.len() as f64;
        let rgb = pixels.iter().map(|p| f64::from(p[0])).sum::<f64>() / pixels.len() as f64;
        eprintln!("homogeneous slab scattering={scattering}: alpha={alpha}, RGB={rgb}, quadrature={expected}");
        assert!((alpha - (1.0 - (-1.0_f64).exp())).abs() < 0.012);
        if scattering {
            assert!((rgb - expected).abs() < 0.012);
        } else {
            assert_eq!(rgb, 0.0);
        }
    }
    let options = JobOptions { samples: 16, lighting: None, volume: None, ..Default::default() };
    let (base, raw) = complete_linear_fixture(executor.context.clone(), options);
    let (zero, same) =
        complete_linear_fixture(executor.context, JobOptions { volume: Some(HomogeneousVolume { extinction_per_mm: 0.0, ..medium }), ..options });
    assert_eq!((base, raw), (zero, same), "zero extinction must not consume extra variates or change old pixels");
}

#[test]
fn gpu_hg_inverse_cdf_and_phase_match_f64_including_nearly_isotropic_media() {
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let source = format!(
        "{SHADER}\n{}",
        r#"
@compute @workgroup_size(64)
fn phase_reference(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x>=4096u {return;}
    let cosine=volume_cosine((f32(gid.x)+0.5)/4096.0);
    accumulation[gid.x]=vec4<f32>(volume_phase(cosine),cosine+1.0,0.0,1.0);
}
"#
    );
    let shader = context.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(source.into()) });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("phase_reference"),
        compilation_options: Default::default(),
        cache: None,
    });
    for g in [-0.8, -0.00005, 0.0, 0.00005, 0.8] {
        let medium = HomogeneousVolume { anisotropy: g, ..slab() };
        let mut task = Task::new(context.clone(), empty(), request(64), settings(medium), CancelToken::default()).unwrap();
        context.queue.write_buffer(&task.uniform, 0, &task.parameters(4096).unwrap());
        let mut encoder = context.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &task.bind, &[]);
            pass.dispatch_workgroups(64, 1, 1);
        }
        context.queue.submit([encoder.finish()]);
        task.sample = 1;
        let pixels = task.read_linear().unwrap();
        let mean = pixels.iter().map(|p| f64::from(p[1]) - 1.0).sum::<f64>() / 4096.0;
        assert!((mean - g).abs() < 1e-5, "g={g} mean={mean}");
        for p in pixels {
            let cosine = (f64::from(p[1]) - 1.0).clamp(-1.0, 1.0);
            let oracle = medium.phase(cosine).unwrap();
            assert!((f64::from(p[0]) - oracle).abs() < oracle * 0.0001 + 1e-6);
        }
    }
}

#[test]
fn gpu_hard_directional_single_scatter_matches_independent_beer_slab_integral() {
    use solidcraft_render::job::{DirectionalLightOptions, DirectionalLights};
    let Some(executor) = executor() else { return };
    let medium = slab();
    let source = DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0, 0.5, 0.25], intensity: 2.0 };
    let options = JobOptions {
        lighting: Some(StudioLighting { directional_lights: DirectionalLights::new(&[source]).unwrap(), ..Default::default() }),
        ..settings(medium)
    };
    let mut task = Task::new(executor.context.clone(), empty(), camera(), options, CancelToken::default()).unwrap();
    while task.sample < options.samples {
        task.complete_sample().unwrap();
    }
    let pixels = task.read_linear().unwrap();
    // Integrate sigma*exp(-sigma*t) camera attenuation times exp(-sigma*t)
    // light attenuation and isotropic phase 1/(4*pi), t in [0,2] mm.
    let expected = 0.8 * 2.0 * (1.0 - (-2.0_f64).exp()) / (8.0 * std::f64::consts::PI);
    for (channel, scale) in [(0, 1.0), (1, 0.5), (2, 0.25)] {
        let mean = pixels.iter().map(|p| f64::from(p[channel])).sum::<f64>() / pixels.len() as f64;
        assert!((mean - expected * scale).abs() < 0.025 * expected * scale, "channel{channel}: {mean} vs {}", expected * scale);
    }
}

// These fixtures distinguish the orthographic optical line from the unchanged
// forward geometry ray. No camera, mesh or render bounds are extended for fog.
#[derive(Clone, Copy, Debug)]
enum OpticalLineEndpoint {
    None,
    Emitter(f64),
    Surface(f64),
}
#[derive(Clone, Copy, Debug)]
struct OpticalLineCase {
    name: &'static str,
    fog: [f64; 2],
    target_z: f64,
    distance: f64,
    endpoint: OpticalLineEndpoint,
    length: f64,
}
fn optical_line_fixture(case: OpticalLineCase) -> (ProductScene, ImageRequest, JobOptions) {
    use solidcraft_render::job::{AreaLight, AreaLights, DirectionalLightOptions, DirectionalLights};
    let mut image = request(4);
    image.camera.target = Vec3::new(0.0, 0.0, case.target_z);
    image.camera.distance = case.distance;
    image.camera.fov = 0.0;
    image.background = [0; 3];
    image.transparent_background = true;
    let mut source = empty();
    let mut lighting = StudioLighting { environment: [0.0; 3], sun: None, ..Default::default() };
    match case.endpoint {
        OpticalLineEndpoint::None => {}
        OpticalLineEndpoint::Emitter(z) => {
            lighting.area_lights =
                AreaLights::new(&[AreaLight { center: [0.0, 0.0, z], u: [1000.0, 0.0, 0.0], v: [0.0, 1000.0, 0.0], radiance: [2.0, 3.0, 5.0] }])
                    .unwrap();
        }
        OpticalLineEndpoint::Surface(z) => {
            let mesh = solidcraft_geom::Mesh {
                positions: vec![
                    Vec3::new(-1000.0, -1000.0, z),
                    Vec3::new(1000.0, -1000.0, z),
                    Vec3::new(1000.0, 1000.0, z),
                    Vec3::new(-1000.0, 1000.0, z),
                ],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            };
            source.bounds = mesh.bounds();
            source.meshes.push(solidcraft_render::ProductMesh {
                name: "Existing forward-only surface".into(),
                mesh: Arc::new(mesh),
                color: [128, 64, 32],
                opacity: 1.0,
                surface: Some(OpaqueSurface::new(Vec3::new(0.5, 0.25, 0.125), 0.0, 0.5, 1.0).unwrap()),
                transmission: None,
                faces: Vec::new(),
            });
            lighting.directional_lights =
                DirectionalLights::new(&[DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: std::f64::consts::PI }])
                    .unwrap();
        }
    }
    let volume = HomogeneousVolume {
        min_mm: [-10000.0, -10000.0, case.fog[0]],
        max_mm: [10000.0, 10000.0, case.fog[1]],
        extinction_per_mm: 1.0 / 128.0,
        scattering_albedo: [0.0; 3],
        anisotropy: 0.0,
    };
    let options =
        JobOptions { samples: 16, max_depth: 1, seed: 71, linear_exr: true, volume: Some(volume), lighting: Some(lighting), ..Default::default() };
    (source, image, options)
}
fn optical_line_collision(pixel: u32, ordinal: u32, seed: u32, optical_depth: f64) -> bool {
    // Independent scalar PCG stream: two camera jitter draws precede the first
    // exponential flight. No production camera/volume/sampler helper is used.
    let mut state = pixel ^ ordinal.wrapping_mul(0x9e3779b9) ^ seed;
    let mut random = || {
        state = state.wrapping_mul(747796405).wrapping_add(2891336453);
        let word = ((state >> ((state >> 28) + 4)) ^ state).wrapping_mul(277803737);
        f64::from((((word >> 22) ^ word) >> 8) as f32 * (1.0 / 16777216.0))
    };
    let _ = random();
    let _ = random();
    let hazard = -(1.0 - random()).ln();
    // Thresholds are independently separated from a GPU log/rounding boundary;
    // coverage remains exact, with no numerical acceptance band around it.
    assert!((hazard - optical_depth).abs() > 1e-5 || optical_depth == 0.0);
    optical_depth > 0.0 && hazard < optical_depth
}
fn optical_line_expected(case: OpticalLineCase, pixel: u32, samples: u32, seed: u32) -> [f64; 4] {
    let hits = (0..samples).filter(|ordinal| optical_line_collision(pixel, *ordinal, seed, case.length / 128.0)).count() as u32;
    let survival = f64::from(samples - hits) / f64::from(samples);
    let visible = match case.endpoint {
        OpticalLineEndpoint::None => false,
        OpticalLineEndpoint::Emitter(z) | OpticalLineEndpoint::Surface(z) => z < case.target_z + case.distance * 4.0,
    };
    let radiance = match case.endpoint {
        OpticalLineEndpoint::Emitter(_) if visible => [2.0, 3.0, 5.0],
        OpticalLineEndpoint::Surface(z) if visible => {
            // Exact normal-incidence Lambert substrate (IOR1 removes specular),
            // pi-radiance delta source, and its independent forward Beer segment.
            let shadow = (case.fog[1] - case.fog[0].max(z)).max(0.0);
            let attenuation = (-shadow / 128.0).exp();
            [0.5 * attenuation, 0.25 * attenuation, 0.125 * attenuation]
        }
        _ => [0.0; 3],
    };
    [radiance[0] * survival, radiance[1] * survival, radiance[2] * survival, if visible { 1.0 } else { f64::from(hits) / f64::from(samples) }]
}
fn optical_line_check(case: OpticalLineCase, samples: u32, seed: u32, pixels: &[[f32; 4]], errors: &mut Vec<String>) {
    assert_eq!(pixels.len(), 16);
    let mut failures = 0;
    for (pixel, actual) in pixels.iter().enumerate() {
        let expected = optical_line_expected(case, pixel as u32, samples, seed);
        let alpha = expected[3] as f32;
        if actual[3] != alpha || (0..3).any(|c| (f64::from(actual[c]) - expected[c]).abs() > 2e-6) {
            failures += 1;
            if errors.len() < 32 {
                errors.push(format!("{} prefix{samples} pixel{pixel}: {actual:?} vs {expected:?}", case.name));
            }
        }
    }
    eprintln!(
        "orthographic signed optical line case={} prefix={samples} length={} failures={failures} first={:?}",
        case.name, case.length, pixels[0]
    );
}

fn optical_line_raw(bytes: &[u8], width: usize, height: usize) -> Vec<[f32; 4]> {
    // Independent uncompressed FLOAT OpenEXR wire reader. The numerical oracle
    // above never calls the production volume, sampler or image codec.
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    assert_eq!((word(0), word(4)), (20_000_630, 2));
    let string = |at: &mut usize| {
        let start = *at;
        *at += bytes[start..].iter().position(|b| *b == 0).unwrap() + 1;
        &bytes[start..*at - 1]
    };
    let mut at = 8;
    let (mut channels, mut window, mut compression) = (false, false, false);
    while bytes[at] != 0 {
        let name = string(&mut at);
        let kind = string(&mut at);
        let len = word(at) as usize;
        at += 4;
        let data = &bytes[at..at + len];
        at += len;
        match name {
            b"channels" => {
                assert_eq!(kind, b"chlist");
                let mut c = 0;
                for name in [b"A", b"B", b"G", b"R"] {
                    assert_eq!(&data[c..c + 2], &[name[0], 0]);
                    c += 2;
                    assert_eq!(u32::from_le_bytes(data[c..c + 4].try_into().unwrap()), 2);
                    assert_eq!(&data[c + 4..c + 8], &[0; 4]);
                    assert_eq!(u32::from_le_bytes(data[c + 8..c + 12].try_into().unwrap()), 1);
                    assert_eq!(u32::from_le_bytes(data[c + 12..c + 16].try_into().unwrap()), 1);
                    c += 16;
                }
                assert_eq!(&data[c..], &[0]);
                channels = true;
            }
            b"compression" => {
                assert_eq!(kind, b"compression");
                assert_eq!(data, &[0]);
                compression = true;
            }
            b"dataWindow" => {
                assert_eq!(kind, b"box2i");
                let bounds: Vec<_> = data.as_chunks::<4>().0.iter().map(|x| i32::from_le_bytes(*x)).collect();
                assert_eq!(bounds, [0, 0, width as i32 - 1, height as i32 - 1]);
                window = true;
            }
            _ => {}
        }
    }
    assert!(channels && window && compression);
    at += 1;
    let mut pixels = vec![[0.0; 4]; width * height];
    for y in 0..height {
        let offset = u64::from_le_bytes(bytes[at + y * 8..at + y * 8 + 8].try_into().unwrap()) as usize;
        assert_eq!((word(offset) as usize, word(offset + 4) as usize), (y, width * 16));
        for x in 0..width {
            for (channel, plane) in [(0, 3), (1, 2), (2, 1), (3, 0)] {
                pixels[y * width + x][channel] = f32::from_bits(word(offset + 8 + (plane * width + x) * 4));
            }
        }
    }
    pixels
}
fn optical_line_finish(task: &mut Task, units: usize) -> (Vec<u8>, Vec<u8>) {
    for _ in 0..20000 {
        if let TaskPoll::Complete(png) = task.step(units).unwrap() {
            let raw = task.take_linear_exr().unwrap();
            assert!(task.take_linear_exr().is_none());
            return (png, raw);
        }
    }
    panic!("bounded optical-line finishing did not complete")
}
fn optical_line_png_alpha(case: OpticalLineCase, samples: u32, seed: u32, png: &[u8], errors: &mut Vec<String>) {
    let image = image::load_from_memory(png).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (4, 4));
    for (pixel, actual) in image.pixels().enumerate() {
        let alpha = optical_line_expected(case, pixel as u32, samples, seed)[3];
        let expected = (alpha * 255.0).round() as u8;
        if actual[3] != expected && errors.len() < 32 {
            errors.push(format!("{} PNG prefix{samples} alpha pixel{pixel}: {} vs {expected}", case.name, actual[3]));
        }
    }
}

#[test]
fn gpu_orthographic_volume_signed_line_absorption_matches_every_complete_prefix() {
    let Some(executor) = executor() else { return };
    let cases = [
        OpticalLineCase {
            name: "empty_origin_inside_slab",
            fog: [300.0, 500.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::None,
            length: 200.0,
        },
        OpticalLineCase {
            name: "empty_box_entirely_before_origin",
            fog: [450.0, 500.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::None,
            length: 50.0,
        },
        OpticalLineCase {
            name: "empty_box_after_origin",
            fog: [-500.0, -300.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::None,
            length: 200.0,
        },
        OpticalLineCase {
            name: "emitter_after_foreground_fog",
            fog: [300.0, 500.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::Emitter(0.0),
            length: 200.0,
        },
        OpticalLineCase {
            name: "emitter_terminates_inside_fog",
            fog: [300.0, 500.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::Emitter(350.0),
            length: 150.0,
        },
        OpticalLineCase {
            name: "opaque_terminates_inside_fog",
            fog: [300.0, 500.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::Surface(350.0),
            length: 150.0,
        },
        OpticalLineCase {
            name: "opaque_before_background_fog",
            fog: [-500.0, -300.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::Surface(0.0),
            length: 0.0,
        },
        OpticalLineCase {
            name: "negative_geometry_hit_stays_invisible",
            fog: [300.0, 500.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::Surface(550.0),
            length: 200.0,
        },
    ];
    let mut errors = Vec::new();
    for case in cases {
        let (source, image, options) = optical_line_fixture(case);
        let original_camera = image.camera;
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        let packed = task.parameters(16).unwrap();
        assert_eq!(f32::from_le_bytes(packed[44..48].try_into().unwrap()), 0.0);
        assert_eq!(f64::from(f32::from_le_bytes(packed[40..44].try_into().unwrap())) + case.target_z, case.target_z + case.distance * 4.0);
        for samples in 1..=options.samples {
            task.complete_sample().unwrap();
            optical_line_check(case, samples, options.seed, &task.read_linear().unwrap(), &mut errors);
        }
        let linear = task.read_linear().unwrap();
        let (png, raw) = optical_line_finish(&mut task, 7);
        let raw = optical_line_raw(&raw, 4, 4);
        assert_eq!(raw, linear, "{} raw master preserves actual final film bits", case.name);
        optical_line_check(case, options.samples, options.seed, &raw, &mut errors);
        optical_line_png_alpha(case, options.samples, options.seed, &png, &mut errors);
        assert_eq!(task.request.camera, original_camera, "fog must not mutate the authored camera");
    }
    assert!(errors.is_empty(), "strict signed optical-line absorption failures: {errors:#?}");
}

// Preserve the existing GPU/shared Display contract: preview PNGs encode the
// exact stored f32 film coverage, then round8bit. Every film prefix is separately
// checked against the independent rational count with exact f32 equality.
fn optical_line_preview_png_alpha(case: OpticalLineCase, samples: u32, seed: u32, png: &[u8], errors: &mut Vec<String>) {
    let image = image::load_from_memory(png).unwrap().to_rgba8();
    assert_eq!(image.dimensions(), (4, 4));
    for (pixel, actual) in image.pixels().enumerate() {
        let alpha = optical_line_expected(case, pixel as u32, samples, seed)[3] as f32;
        let expected = (f64::from(alpha) * 255.0).round() as u8;
        if actual[3] != expected && errors.len() < 32 {
            errors.push(format!("{} PNG prefix{samples} alpha pixel{pixel}: {} vs {expected}", case.name, actual[3]));
        }
    }
}

#[test]
fn gpu_orthographic_volume_axial_origin_changes_preserve_lines_framing_and_raw_prefixes() {
    let Some(executor) = executor() else { return };
    let mut baseline = None;
    let mut baseline_frame: Option<Vec<u8>> = None;
    let mut errors = Vec::new();
    for (target_z, distance) in [(-200.0, 100.0), (0.0, 100.0), (200.0, 100.0), (0.0, 50.0), (0.0, 150.0)] {
        let case = OpticalLineCase {
            name: "translated_or_rescaled_infinite_line",
            fog: [300.0, 500.0],
            target_z,
            distance,
            endpoint: OpticalLineEndpoint::Emitter(0.0),
            length: 200.0,
        };
        let (source, image, options) = optical_line_fixture(case);
        let camera = image.camera;
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        let packet = task.parameters(16).unwrap();
        assert_eq!(f32::from_le_bytes(packet[48..52].try_into().unwrap()), (distance * 0.4) as f32);
        if let Some(frame) = &baseline_frame {
            if distance == 100.0 {
                assert_eq!(&packet[48..96], frame.as_slice(), "axial target translation keeps exact right/up/back and orthographic framing");
            } else {
                assert_ne!(
                    &packet[48..80],
                    &frame[..32],
                    "real distance change rescales the orthographic span; only uniform-coverage radiance is invariant"
                );
            }
        } else {
            baseline_frame = Some(packet[48..96].to_vec());
        }
        let mut prefixes = Vec::new();
        for count in 1..=options.samples {
            task.complete_sample().unwrap();
            let pixels = task.read_linear().unwrap();
            optical_line_check(case, count, options.seed, &pixels, &mut errors);
            prefixes.push(pixels);
        }
        let (png, raw) = optical_line_finish(&mut task, 7);
        optical_line_check(case, options.samples, options.seed, &optical_line_raw(&raw, 4, 4), &mut errors);
        assert_eq!(task.request.camera, camera);
        let output = (prefixes, png, raw);
        if let Some(baseline) = &baseline {
            assert_eq!(&output, baseline, "only the arbitrary launch-plane location or homogeneous coverage scale changed");
        } else {
            baseline = Some(output);
        }
    }
    assert!(errors.is_empty(), "translated signed optical-line errors: {errors:#?}");
}

#[test]
fn gpu_orthographic_volume_every_preview_raw_and_cancel_are_quantum_safe() {
    let Some(executor) = executor() else { return };
    let case = OpticalLineCase {
        name: "quantum_complete_prefixes",
        fog: [300.0, 500.0],
        target_z: 0.0,
        distance: 100.0,
        endpoint: OpticalLineEndpoint::None,
        length: 200.0,
    };
    let run = |units| {
        let (source, image, options) = optical_line_fixture(case);
        let mut task = Task::new(executor.context.clone(), source, image, options, CancelToken::default()).unwrap();
        task.preview_enabled = true;
        let (mut prefixes, mut previews, mut uniforms, mut errors) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for _ in 0..20000 {
            if task.sample < options.samples && uniforms.len() == task.sample as usize {
                uniforms.push(task.parameters(16).unwrap());
            }
            let before = task.sample;
            let poll = task.step(units).unwrap();
            if task.sample > before {
                assert_eq!(task.sample, before + 1);
                let pixels = task.read_linear().unwrap();
                optical_line_check(case, task.sample, options.seed, &pixels, &mut errors);
                prefixes.push(pixels);
            }
            if let Some(preview) = task.take_preview() {
                assert_eq!(preview.samples_per_pixel, previews.len() as u32 + 1);
                optical_line_preview_png_alpha(case, preview.samples_per_pixel, options.seed, &preview.png, &mut errors);
                previews.push(preview.png);
            }
            if let TaskPoll::Complete(png) = poll {
                let raw = task.take_linear_exr().unwrap();
                assert!(task.take_linear_exr().is_none());
                assert_eq!((prefixes.len(), previews.len(), uniforms.len()), (16, 15, 16));
                assert_eq!(optical_line_raw(&raw, 4, 4), *prefixes.last().unwrap());
                optical_line_png_alpha(case, 16, options.seed, &png, &mut errors);
                assert!(errors.is_empty(), "quantum{units}: {errors:#?}");
                return (prefixes, previews, uniforms, png, raw);
            }
        }
        panic!("bounded progressive volume output did not complete")
    };
    assert_eq!(run(1), run(16384), "all completed previews, full input packets, PNG/raw and film prefixes survive output quantum changes exactly");
    let (source, mut image, options) = optical_line_fixture(case);
    image.width = 16;
    image.height = 16;
    let token = CancelToken::default();
    let mut task = Task::new(executor.context, source, image, options, token.clone()).unwrap();
    task.preview_enabled = true;
    task.dispatch().unwrap();
    assert_eq!(task.sample, 0);
    assert!(task.offset > 0 && task.offset < 256, "cancel a real partial pass");
    token.cancel();
    assert_eq!(task.step(1).err().unwrap().code, ErrorCode::Cancelled);
    assert!(task.take_preview().is_none() && task.take_linear_exr().is_none());
}

#[test]
fn gpu_orthographic_volume_cancel_releases_work_before_exact_retained_metadata() {
    use solidcraft_render::job::{JobId, Phase};
    let Some(executor) = executor() else { return };
    let host = ImageHost::with_executor(Arc::new(executor)).unwrap();
    let case = OpticalLineCase {
        name: "real_host_cancel",
        fog: [300.0, 500.0],
        target_z: 0.0,
        distance: 100.0,
        endpoint: OpticalLineEndpoint::None,
        length: 200.0,
    };
    let (source, mut image, mut options) = optical_line_fixture(case);
    image.width = 128;
    image.height = 128;
    options.samples = 4096;
    let retained = options.retained_bytes().unwrap() + image.retained_resource_metadata_bytes();
    let admission = host.admit(&source, &image, options).unwrap();
    assert!(admission.estimated_total_bytes > retained);
    assert_eq!(host.admitted_bytes(), 0, "preflight leaves no reserved work");
    let handle = host.submit_with_options(JobId { session: 918, serial: 1, generation: 1 }, 1, source, image, options).unwrap();
    handle.cancel().unwrap();
    let status = handle.wait_timeout(std::time::Duration::from_secs(30)).unwrap();
    assert_eq!(status.progress.phase, Phase::Cancelled);
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while !handle.work_released() {
        assert!(Instant::now() < deadline, "cancelled GPU volume work did not drain");
        std::thread::yield_now();
    }
    assert!(handle.frame().unwrap().is_none() && handle.preview().unwrap().is_none());
    assert_eq!(host.admitted_bytes(), retained, "only actual options and fixed request headers remain owned by Record");
    drop(handle);
    while host.admitted_bytes() != 0 {
        assert!(Instant::now() < deadline, "last Record did not release retained metadata");
        std::thread::yield_now();
    }
    assert_eq!(host.admitted_bytes(), 0);
}

#[test]
fn gpu_volume_signed_primary_preserves_old_perspective_lens_none_and_zero_bytes() {
    use solidcraft_engine::doc::render_resources::HdrId;
    let Some(executor) = executor() else { return };
    let context = &executor.context;
    let old_hook = r#"            let segment=volume_segment(o+o_low,d,min(hit.distance,emitter_distance));"#;
    let new_hook = r#"            var segment=vec2<f32>(0.0);
            if bounce==0u && u.eye.w<=0.5 {
                // The orthographic camera has no physical eye plane. Its fog
                // line starts at the finite box entry, even before the artificial
                // geometry origin; the existing forward endpoint stays fixed.
                segment=volume_orthographic_segment(o+o_low,d,min(hit.distance,emitter_distance));
            } else {
                segment=volume_segment(o+o_low,d,min(hit.distance,emitter_distance));
            }"#;
    let helper = r#"// Only the primary orthographic optical line uses signed entry. Secondary,
// shadow and perspective rays retain volume_segment's physical forward origin.
// The caller supplies the original forward geometry/emitter endpoint unchanged.
fn volume_orthographic_segment(o: vec3<f32>, d: vec3<f32>, limit: f32) -> vec2<f32> {
    var enter = -FAR;
    var leave = limit;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        if d[axis] == 0.0 {
            if o[axis] < u.volume_min[axis] || o[axis] > u.volume_max[axis] { return vec2<f32>(0.0); }
        } else {
            let a = (u.volume_min[axis]-o[axis])/d[axis];
            let b = (u.volume_max[axis]-o[axis])/d[axis];
            enter=max(enter,min(a,b));
            leave=min(leave,max(a,b));
        }
        if enter >= leave { return vec2<f32>(0.0); }
    }
    return vec2<f32>(enter,leave);
}
"#;
    assert_eq!(SHADER.matches(new_hook).count(), 1);
    assert_eq!(SHADER.matches(helper).count(), 1);
    let helper_block = format!("\n{helper}");
    let helper_offset = SHADER.find(&helper_block).unwrap();
    let original = SHADER.replacen(new_hook, old_hook, 1).replacen(&helper_block, "", 1);
    assert_eq!(original.matches(old_hook).count(), 1);
    let mut restored = original.replacen(old_hook, new_hook, 1);
    restored.insert_str(helper_offset, &helper_block);
    assert_eq!(restored, SHADER, "the whole-source inverse restores only the signed primary hook/helper");
    // Historical a4f69bd8 is the archived pre-GPU12 source. This current control
    // is independently reconstructed from 96190d, the 35af rename and the early wrapper.
    assert_eq!(
        HdrId::of(original.as_bytes()).to_string(),
        "f373400816e40e803691ceb847d8c9d7983fd4744c3874a322363810c14b6d6c",
        "exact composed pre-fog shader independently reconstructed; no unrelated producer math edited"
    );
    let creating = scopes(&context.device);
    let module = context.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("exact old signed-line control"),
        source: wgpu::ShaderSource::Wgsl(original.into()),
    });
    let layout = context.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&context.layout)],
        immediate_size: 0,
    });
    let pipeline = context.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    checked(creating).unwrap();
    warm_up(&context.device, &context.queue, &pipeline, &placeholder_bind(&context.device, &context.layout, 7).unwrap(), &context.healthy).unwrap();
    let mut inactive_baseline = None;
    for mode in 0..8 {
        let case = OpticalLineCase {
            name: "unchanged_routes",
            fog: [300.0, 500.0],
            target_z: 0.0,
            distance: 100.0,
            endpoint: OpticalLineEndpoint::Emitter(0.0),
            length: 0.0,
        };
        let (source, mut image, mut options) = optical_line_fixture(case);
        match mode {
            0 => options.volume = None,
            1 => options.volume.as_mut().unwrap().extinction_per_mm = 0.0,
            2 => image.camera.fov = 0.4,
            3 => {
                image.camera.fov = 0.4;
                options.lens = Some(solidcraft_render::job::ThinLens { aperture_radius_mm: 2.0, focus_distance_mm: 100.0 });
            }
            4 => {
                let volume = options.volume.as_mut().unwrap();
                volume.min_mm[2] = 100.0;
                volume.max_mm[2] = 300.0;
            }
            5 => {
                let volume = options.volume.as_mut().unwrap();
                volume.min_mm[0] = 1000.0;
                volume.max_mm[0] = 2000.0;
            }
            6 => {
                image.camera.fov = 0.4;
                let volume = options.volume.as_mut().unwrap();
                volume.min_mm[2] = 0.0;
                volume.max_mm[2] = 200.0;
            }
            7 => {
                image.camera.fov = 0.4;
                options.lens = Some(solidcraft_render::job::ThinLens { aperture_radius_mm: 2.0, focus_distance_mm: 100.0 });
                let volume = options.volume.as_mut().unwrap();
                volume.min_mm[2] = 0.0;
                volume.max_mm[2] = 200.0;
            }
            _ => panic!("unexpected unchanged-volume fixture mode"),
        }
        let run = |old: bool| {
            let mut task = Task::new(context.clone(), source.clone(), image.clone(), options, CancelToken::default()).unwrap();
            let (mut uniforms, mut prefixes) = (Vec::new(), Vec::new());
            for sample in 0..options.samples {
                assert_eq!((task.sample, task.offset, task.lane), (sample, 0, 0));
                let params = task.parameters(16).unwrap();
                uniforms.push(params.clone());
                if old {
                    let checking = scopes(&context.device);
                    context.queue.write_buffer(&task.uniform, 0, &params);
                    let mut encoder = context.device.create_command_encoder(&Default::default());
                    {
                        let mut pass = encoder.begin_compute_pass(&Default::default());
                        pass.set_pipeline(&pipeline);
                        pass.set_bind_group(0, &task.bind, &[]);
                        pass.dispatch_workgroups(16u32.div_ceil(PATH_WORKGROUP_SIZE), 1, 1);
                    }
                    let submission = context.queue.submit([encoder.finish()]);
                    checked(checking).unwrap();
                    context.wait(submission, TIMEOUT).unwrap();
                    task.sample += 1;
                } else {
                    task.complete_sample().unwrap();
                }
                prefixes.push(task.read_linear().unwrap());
            }
            let (png, raw) = optical_line_finish(&mut task, 7);
            assert_eq!(optical_line_raw(&raw, 4, 4), *prefixes.last().unwrap());
            (uniforms, prefixes, png, raw)
        };
        let old = run(true);
        let new = run(false);
        assert_eq!(old, new, "mode{mode}: complete input packets, each film prefix and PNG/raw must remain byte-identical");
        if mode == 0 {
            inactive_baseline = Some(new);
        } else if mode == 1 {
            assert_eq!(inactive_baseline.as_ref().unwrap(), &new, "Some(0) preserves full None uniforms, stream and outputs exactly");
        }
    }
}
