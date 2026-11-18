//! Manual timestamp measurement: model-only AO versus the virtual-ground receiver.
//! Run explicitly with --ignored, WGPU_BACKEND and SOLIDCRAFT_GPU=hardware|software.
//! SOLIDCRAFT_BENCH_OUTPUT_DIR is the folder for JSON evidence. Not a CI speed threshold.
#![cfg(not(target_arch = "wasm32"))]
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_engine::geom::Aabb3;
use solidcraft_geom::MeshGeom;
use solidcraft_gpu as gpu;
use solidcraft_viewport::Camera;
use solidcraft_viewport_gpu::view::{Frame, GpuScene, GpuTarget, Renderer};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[test]
#[ignore = "manual CPU/GPU measurement; requires a timestamp-capable adapter"]
fn measure_ground_ao_renderer_cost() -> Result<(), Box<dyn std::error::Error>> {
    ao_comparison(0)?;
    ao_comparison(1)
}
fn ao_comparison(ground: u32) -> Result<(), Box<dyn std::error::Error>> {
    let (adapter, _) = solidcraft_viewport_gpu::select::pick(gpu::env::policy().map_err(std::io::Error::other)?).ok_or("no matching adapter")?;
    let info = adapter.get_info();
    let features = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    if !adapter.features().contains(features) {
        return Err("adapter has no encoder timestamps".into());
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("existing AO comparison"),
        required_features: features,
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let target = GpuTarget {
        format: wgpu::TextureFormat::Rgba8Unorm,
        depth: Some(wgpu::TextureFormat::Depth24PlusStencil8),
        samples: 4,
        tess: false,
        oit: false,
    };
    let mut session = solidcraft_engine::Session::default();
    for corner in [[-65, -10, 0], [45, -10, 0]] {
        session.execute("solid.box", &json!({"length":20,"width":20,"height":20,"corner":corner,"operation":"new"}))?;
    }
    session.execute("solid.torus", &json!({"major":14,"minor":4,"center":[0,0,9],"operation":"new"}))?;
    let meshes: Vec<_> = session.world_state()?.bodies.iter().map(|b| b.mesh()).collect();
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |bounds, mesh| bounds.union(&mesh.bounds()));
    let radius = (bounds.diagonal() * 0.04).clamp(1e-4, 1e6) as f32;
    let cam = Camera {
        target: Vec3::new(8.379000844961002, 2.6776676413414724, 7.806862777456871),
        distance: 131.41653316382525,
        yaw: -0.5,
        pitch: 0.65,
        ..Default::default()
    };
    let query =
        device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("existing AO timestamps"), ty: wgpu::QueryType::Timestamp, count: 4 });
    let buffer = |usage| device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 32, usage, mapped_at_creation: false });
    let resolve = buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
    let read = buffer(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
    let software = info.device_type == wgpu::DeviceType::Cpu;
    let samples = if software { 12 } else { 30 };
    let warm = 5;
    let mut rows = Vec::new();
    for (width, height) in [(1670u32, 838u32), (3340, 1676)] {
        if software && width > 1670 {
            continue;
        }
        let mut scene = GpuScene::default();
        let color = scene.color([190, 195, 205, 255]);
        for mesh in &meshes {
            for tri in &mesh.triangles {
                for index in tri {
                    let p = mesh.positions.get(*index as usize).ok_or("bad index")?;
                    let n = mesh.normals.get(*index as usize).copied().unwrap_or(Vec3::Z);
                    scene.tri(p.to_f32(), n.to_f32(), color);
                }
            }
        }
        if ground > 0 {
            receiver(&mut scene, bounds_in_frame(&[bounds], &cam), &cam, f64::from(radius));
        }
        let mut frame = comparison_frame(scene, &cam, width, height, 1);
        let mut renderer = Renderer::new(&device, target, None);
        let attachment = |format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("AO comparison target"),
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 4,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let color = attachment(target.format);
        let depth = attachment(wgpu::TextureFormat::Depth24PlusStencil8);
        // Keep each state stable after warm-up: disabling releases the depth image, so alternating
        // every frame would benchmark repeated allocation rather than normal steady AO use.
        for enabled in [false, true, false, true] {
            frame.ao_radius = if enabled { radius } else { 0.0 };
            let mut cpu = Vec::new();
            let mut prepass = Vec::new();
            let mut shade = Vec::new();
            let mut total = Vec::new();
            for sample in 0..samples + warm {
                let mut encoder = device.create_command_encoder(&Default::default());
                encoder.write_timestamp(&query, 0);
                let started = Instant::now();
                renderer.prepare(&device, &queue, &mut encoder, &frame);
                let cpu_ms = started.elapsed().as_secs_f64() * 1000.0;
                encoder.write_timestamp(&query, 1);
                encoder.write_timestamp(&query, 2);
                {
                    let mut pass = encoder
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("existing AO shaded model"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &color,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                                view: &depth,
                                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                            }),
                            ..Default::default()
                        })
                        .forget_lifetime();
                    renderer.paint(&frame, &mut pass, [0.0, 0.0, width as f32, height as f32]);
                }
                encoder.write_timestamp(&query, 3);
                encoder.resolve_query_set(&query, 0..4, &resolve, 0);
                encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 32);
                let submitted = queue.submit([encoder.finish()]);
                read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
                device.poll(wgpu::PollType::Wait { submission_index: Some(submitted), timeout: Some(Duration::from_secs(30)) })?;
                let mapped = read.slice(..).get_mapped_range()?;
                let times: Vec<_> =
                    mapped.as_chunks::<8>().0.iter().map(|b| u64::from_le_bytes(*b) as f64 * f64::from(queue.get_timestamp_period()) / 1e6).collect();
                if sample >= warm {
                    let at = |i| times.get(i).copied().ok_or("missing timestamp");
                    cpu.push(cpu_ms);
                    prepass.push((at(1)? - at(0)?).max(0.0));
                    shade.push((at(3)? - at(2)?).max(0.0));
                    total.push((at(3)? - at(0)?).max(0.0));
                }
                drop(mapped);
                read.unmap();
            }
            rows.push(json!({"enabled":enabled,"width":width,"height":height,"samples":samples,
                "cpu_cached_prepare_ms":summary(&mut cpu),"gpu_prepare_ms":summary(&mut prepass),
                "gpu_model_pass_ms":summary(&mut shade),"gpu_total_ms":summary(&mut total)}));
        }
    }
    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(error.to_string().into());
    }
    let result = json!({"adapter":info.name,"backend":format!("{:?}",info.backend),"radius":radius,"ground":ground,
        "triangles":meshes.iter().map(|m|m.triangles.len()).sum::<usize>(),"rows":rows,
        "notes":"Actual Renderer, same cached opaque model, MSAA4, two off/on pairs after warm-up. ground=0: surface AO; 1: virtual floor; Floor composites require AO on. Projection is not part of this measurement. Stored MSAA4 attachments prevent unused-output elimination; presentation, MSAA resolve and final downsample are excluded. 3340x1676 is SSAA2 working resolution. GPU total includes prepare plus model/clear, not the application frame."});
    let label = match ground {
        1 => "ao-ground",
        2 => "ao-ground-projection",
        _ => "ao-baseline",
    };
    let output =
        std::env::var_os("SOLIDCRAFT_BENCH_OUTPUT_DIR").map(std::path::PathBuf::from).ok_or("set SOLIDCRAFT_BENCH_OUTPUT_DIR to an output folder")?;
    std::fs::create_dir_all(&output)?;
    std::fs::write(output.join(format!("{label}-comparison.json")), serde_json::to_vec_pretty(&result)?)?;
    println!("{label} comparison completed: {} {:?}", info.name, info.backend);
    Ok(())
}

fn summary(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    json!({"median":values.get(values.len()/2),"p95":values.get(values.len().saturating_mul(95)/100),"samples":values.len()})
}

fn bounds_in_frame(bounds: &[Aabb3], cam: &Camera) -> Aabb3 {
    let mut combined = Aabb3::EMPTY;
    for b in bounds.iter().filter(|b| !b.is_empty()) {
        for i in 0..8 {
            combined.add(cam.to_frame(Vec3::new(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            )));
        }
    }
    combined
}

fn comparison_frame(scene: GpuScene, cam: &Camera, width: u32, height: u32, key: u64) -> Frame {
    let empty = || Arc::new(std::sync::Mutex::new(None));
    Frame {
        product_lights: None,
        upload_error: None,
        product: false,
        key,
        slot: Arc::new(std::sync::Mutex::new(Some(scene))),
        sketch_key: 0,
        sketch_slot: empty(),
        hl_key: 0,
        hl_slot: empty(),
        pv_key: 0,
        pv_slot: empty(),
        pv_hide: false,
        preview_bounds: None,
        view_proj: cam.view_proj(f64::from(width) / f64::from(height), 200.0).to_f32(),
        turned_depth: None,
        back: cam.back().to_f32(),
        size_px: [width as f32, height as f32],
        clip: [[0.0; 4]; solidcraft_viewport_gpu::view::MAX_SECTION_PLANES],
        clip_hatch: [0; solidcraft_viewport_gpu::view::MAX_SECTION_PLANES],
        cap: [0.0; 4],
        images: Vec::new(),
        clouds: Vec::new(),
        analysis: [0.0; 8],
        analysis_colors: [0.0; 36],
        access: None,
        outline: false,
        eye: [0.0; 4],
        ao_radius: 0.0,
        ao_samples: solidcraft_viewport_gpu::view::AO_SAMPLES,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        hatch: Default::default(),
        sky: [0.0; 4],
        ssaa: 1.0,
        moving: None,
    }
}

fn receiver(sc: &mut GpuScene, b: Aabb3, cam: &Camera, radius: f64) {
    sc.ground_ao = solidcraft_viewport_gpu::view::GroundAo::new(
        solidcraft_render::view_bounds(&b),
        b.min.z - 0.001 * (1.0 + b.diagonal()),
        cam.up,
        radius as f32,
    );
}
