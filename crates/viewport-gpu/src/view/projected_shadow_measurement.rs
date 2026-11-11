//! Explicit manual measurement against the original ellipse; ignored by normal CI.
//! Set WGPU_BACKEND, VIEWPORT_TEST_GPU=hardware|software and SOLIDCRAFT_BENCH_OUTPUT_DIR.
//! The split viewport uses procedural boxes and a torus with fixed subdivisions.
//! Earlier timings used CAD-generated meshes and remain historical.
use super::super::{BlockBytes, Frame, GpuScene, GpuTarget, Renderer, SceneBlock};
use super::*;
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// Isolate mask refresh with existing vertex buffers, including an input beyond the old
/// prototype's 500,000-triangle cutoff. The overlapping copies stress overdraw deliberately.
#[test]
#[ignore = "manual large-mesh timestamp measurement"]
fn measure_projected_shadow_large_mesh() -> Result<(), Box<dyn std::error::Error>> {
    let (adapter, _) = crate::select::pick(crate::test_env::policy()).ok_or("no adapter")?;
    let flags = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    if !adapter.features().contains(flags) {
        return Err("no encoder timestamps".into());
    }
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: flags,
        required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
        ..Default::default()
    }))?;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut model = GpuScene::default();
    let color = model.color([190, 195, 205, 255]);
    let bounds = append_torus(&mut model, color);
    let triangle_count = model.tris.len() / (3 * super::super::TRI_SIZE);
    let cam = Camera::default();
    let query = device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: 2 });
    let buffer = |usage| device.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage, mapped_at_creation: false });
    let resolve = buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC);
    let read = buffer(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ);
    let mut rows = Vec::new();
    let large = 500_000usize / triangle_count + 1;
    for copies in [1usize, large] {
        let body_bytes = model.tris.len() * copies;
        let block = SceneBlock {
            key: 77,
            bytes: Arc::new(BlockBytes {
                vertices: Arc::new(model.tris.repeat(copies)),
                opaque: Arc::new(std::iter::once(0..u32::try_from(body_bytes / super::super::TRI_SIZE)?).collect()),
                ..Default::default()
            }),
        };
        for projected in [false, true] {
            let mut renderer =
                Renderer::new(&device, GpuTarget { format: wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false }, None);
            let mut cpu = Vec::new();
            let mut gpu = Vec::new();
            let mut cold = None;
            for sample in 0..15 {
                let mut scene = GpuScene { palette: model.palette.clone(), ..Default::default() };
                scene.blocks.push(block.clone());
                let started = Instant::now();
                if projected {
                    scene.projected_shadow = Some(Arc::new(
                        Projection::new(bounds, bounds.min.z - 0.001 * (1.0 + bounds.diagonal()), &cam, 0.3, Vec::new())
                            .ok_or("projection rejected")?,
                    ));
                }
                let metadata_ms = started.elapsed().as_secs_f64() * 1000.0;
                let frame = comparison_frame(scene, &cam, 1670, 838, sample + 1);
                let mut encoder = device.create_command_encoder(&Default::default());
                encoder.write_timestamp(&query, 0);
                let started = Instant::now();
                renderer.prepare(&device, &queue, &mut encoder, &frame);
                let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
                encoder.write_timestamp(&query, 1);
                encoder.resolve_query_set(&query, 0..2, &resolve, 0);
                encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 16);
                let submitted = queue.submit([encoder.finish()]);
                read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
                device.poll(wgpu::PollType::Wait { submission_index: Some(submitted), timeout: Some(Duration::from_secs(30)) })?;
                let mapped = read.slice(..).get_mapped_range()?;
                let times: Vec<_> = mapped.as_chunks::<8>().0.iter().map(|t| u64::from_le_bytes(*t)).collect();
                let gpu_ms = (times[1] - times[0]) as f64 * f64::from(queue.get_timestamp_period()) / 1e6;
                drop(mapped);
                read.unmap();
                if sample == 0 {
                    cold = Some(json!({"cpu_prepare_ms":prepare_ms,"gpu_prepare_ms":gpu_ms}));
                }
                if sample >= 3 {
                    cpu.push(metadata_ms + prepare_ms);
                    gpu.push(gpu_ms);
                }
                assert_eq!(renderer.shadow.is_some(), projected, "large input silently lost its projection");
                let bytes: u64 = renderer.model.blocks.values().filter_map(|b| b.vertices.as_ref()).map(wgpu::Buffer::size).sum();
                assert_eq!(bytes, body_bytes as u64, "same existing model buffer size for both modes");
            }
            rows.push(json!({"copies":copies,"triangles":triangle_count*copies,"body_buffer_bytes":body_bytes,
                "projected":projected,"cpu_metadata_and_cached_prepare_ms":summary(&mut cpu),"gpu_refresh_ms":summary(&mut gpu),"first_use":cold,
                "extra_mask_texture_bytes":if projected {SIZE*SIZE*12}else{0}}));
        }
    }
    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(error.to_string().into());
    }
    let output = std::env::var_os("SOLIDCRAFT_BENCH_OUTPUT_DIR").map(std::path::PathBuf::from).ok_or("set SOLIDCRAFT_BENCH_OUTPUT_DIR")?;
    std::fs::create_dir_all(&output)?;
    let info = adapter.get_info();
    std::fs::write(
        output.join("projected-shadow-large-mesh.json"),
        serde_json::to_vec_pretty(&json!({"adapter":info.name,"backend":format!("{:?}",info.backend),"rows":rows,
        "fixture":"Procedural torus, 64 major by 32 minor subdivisions, with overlapping copies. Earlier timings used CAD-generated meshes.",
        "notes":"Prepare-only refresh, same cached model buffers; excludes opaque model paint and engine recompute. Overlapping torus copies intentionally stress mask overdraw beyond the former 500k cap. First use includes model upload and lazy shadow pipeline creation."}))?,
    )?;
    Ok(())
}
#[test]
#[ignore = "manual timestamp measurement; no CI performance threshold"]
fn measure_projected_shadow_cost() -> Result<(), Box<dyn std::error::Error>> {
    let (adapter, _) = crate::select::pick(crate::test_env::policy()).ok_or("no matching adapter")?;
    let info = adapter.get_info();
    let flags = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let timestamps = adapter.features().contains(flags);
    let inside = timestamps && adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
    let features = if timestamps { flags } else { wgpu::Features::empty() }
        | if inside { wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES } else { wgpu::Features::empty() };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("ellipse comparison"),
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
    let cam = Camera {
        target: Vec3::new(8.379000844961002, 2.6776676413414724, 7.806862777456871),
        distance: 131.41653316382525,
        yaw: -0.5,
        pitch: 0.65,
        ..Default::default()
    };
    let mut geometry = GpuScene::default();
    let color = geometry.color([190, 195, 205, 255]);
    let size = Vec3::new(20.0, 20.0, 20.0);
    let left = Vec3::new(-65.0, -10.0, 0.0);
    let right = Vec3::new(45.0, -10.0, 0.0);
    let left_box = Bounds { min: left, max: left + size };
    let right_box = Bounds { min: right, max: right + size };
    append_box(&mut geometry, left_box, color);
    append_box(&mut geometry, right_box, color);
    let bounds = [left_box, right_box, append_torus(&mut geometry, color)];
    let vertex_count = geometry.tris.len() / super::super::TRI_SIZE;
    let model_block = SceneBlock {
        key: 77,
        bytes: Arc::new(BlockBytes {
            opaque: Arc::new(std::iter::once(0..u32::try_from(vertex_count)?).collect()),
            vertices: Arc::new(geometry.tris),
            ..Default::default()
        }),
    };
    let make_scene = |kind: u32| -> Result<GpuScene, Box<dyn std::error::Error>> {
        let mut sc = GpuScene { palette: geometry.palette.clone(), ..Default::default() };
        sc.blocks.push(model_block.clone());
        let b = bounds_in_frame(&bounds, &cam);
        if kind > 0 {
            ellipse(&mut sc, b, &cam, 80);
        }
        if kind == 2 {
            let fallback = std::mem::take(&mut sc.shadow);
            sc.projected_shadow = Some(Arc::new(
                Projection::new(b, b.min.z - 0.001 * (1.0 + b.diagonal()), &cam, 80.0 / 255.0 * 0.8, fallback).ok_or("invalid projection")?,
            ));
        }
        Ok(sc)
    };
    let query = timestamps.then(|| {
        device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("ellipse comparison times"), ty: wgpu::QueryType::Timestamp, count: 4 })
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 32,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let read = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 32,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let finish = |mut encoder: wgpu::CommandEncoder| -> Result<Option<Vec<f64>>, Box<dyn std::error::Error>> {
        if let Some(q) = &query {
            encoder.resolve_query_set(q, 0..4, &resolve, 0);
            encoder.copy_buffer_to_buffer(&resolve, 0, &read, 0, 32);
        }
        let submitted = queue.submit([encoder.finish()]);
        if timestamps {
            read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        }
        device.poll(wgpu::PollType::Wait { submission_index: Some(submitted), timeout: Some(Duration::from_secs(30)) })?;
        if !timestamps {
            return Ok(None);
        }
        let mapped = read.slice(..).get_mapped_range()?;
        let raw: Vec<_> =
            mapped.as_chunks::<8>().0.iter().map(|b| u64::from_le_bytes(*b) as f64 * f64::from(queue.get_timestamp_period()) / 1e6).collect();
        drop(mapped);
        read.unmap();
        Ok(Some(raw))
    };
    let sample_time = |times: &Option<Vec<f64>>, a: usize, b: usize| -> Option<f64> {
        let values = times.as_ref()?;
        Some((values.get(b)? - values.get(a)?).max(0.0))
    };
    let samples = if info.device_type == wgpu::DeviceType::Cpu { 12 } else { 30 };
    let warm = 5;
    let mut cached_rows = Vec::new();
    let mut update_rows = Vec::new();
    let mut pipeline_rows = Vec::new();
    for (width, height) in [(1670u32, 838u32), (3340, 1676)] {
        let model = GpuScene { tris: model_block.bytes.vertices.as_ref().clone(), palette: geometry.palette.clone(), ..Default::default() };
        let body_frame = comparison_frame(model, &cam, width, height, 1);
        let mut body_renderer = Renderer::new(&device, target, None);
        let mut encoder = device.create_command_encoder(&Default::default());
        body_renderer.prepare(&device, &queue, &mut encoder, &body_frame);
        let submission = queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(Duration::from_secs(30)) })?;
        let attachment = |format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("ellipse comparison target"),
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
        for (kind, label) in [(0, "off"), (1, "ellipse"), (2, "projected")] {
            let init = Instant::now();
            let mut renderer = Renderer::new(&device, target, None);
            pipeline_rows.push(json!({"kind":label,"width":width,"renderer_pipeline_cpu_ms":init.elapsed().as_secs_f64()*1000.0}));
            if width == 1670 {
                let mut geometry = Vec::new();
                let mut prepare = Vec::new();
                let mut wall = Vec::new();
                let mut gpu = Vec::new();
                let mut input_bytes = 0;
                for sample in 0..samples + warm {
                    let start = Instant::now();
                    let sc = make_scene(kind)?;
                    input_bytes = sc.shadow.len() + sc.projected_shadow.as_ref().map_or(0, |p| p.fallback.len() + std::mem::size_of::<Projection>());
                    let geometry_ms = start.elapsed().as_secs_f64() * 1000.0;
                    let frame = comparison_frame(sc, &cam, width, height, sample as u64 + 2);
                    let started = Instant::now();
                    let mut encoder = device.create_command_encoder(&Default::default());
                    if let Some(q) = &query {
                        encoder.write_timestamp(q, 0);
                    }
                    renderer.prepare(&device, &queue, &mut encoder, &frame);
                    // Draw the shared opaque model separately, then time only the production shadow buffers.
                    renderer.model.tris = None;
                    renderer.model.block_order.clear();
                    let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
                    if let Some(q) = &query {
                        encoder.write_timestamp(q, 1);
                        encoder.write_timestamp(q, 2);
                        encoder.write_timestamp(q, 3);
                    }
                    let times = finish(encoder)?;
                    if sample >= warm {
                        geometry.push(geometry_ms);
                        prepare.push(prepare_ms);
                        wall.push(start.elapsed().as_secs_f64() * 1000.0);
                        if let Some(t) = sample_time(&times, 0, 1) {
                            gpu.push(t);
                        }
                    }
                }
                update_rows.push(json!({"kind":label,"samples":samples,"cpu_geometry_ms":summary(&mut geometry),
                    "cpu_prepare_ms":summary(&mut prepare),"submit_wait_total_ms":summary(&mut wall),"gpu_encoder_region_ms":summary(&mut gpu),
                    "input_bytes":input_bytes,"shadow_texture_bytes":if kind>=2 {256usize.pow(2)*12}else{0}}));
            }
            let frame = comparison_frame(make_scene(kind)?, &cam, width, height, 1000);
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.prepare(&device, &queue, &mut encoder, &frame);
            // Draw the shared opaque model separately, then time only the production shadow buffers.
            renderer.model.tris = None;
            renderer.model.block_order.clear();
            let submission = queue.submit([encoder.finish()]);
            device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(Duration::from_secs(30)) })?;
            let cache_generation = renderer.shadow.as_ref().map_or(0, |s| s.generation);
            let mut full = Vec::new();
            let mut shadow = Vec::new();
            let mut prepare = Vec::new();
            let mut record = Vec::new();
            for sample in 0..samples + warm {
                let mut encoder = device.create_command_encoder(&Default::default());
                let started = Instant::now();
                renderer.prepare(&device, &queue, &mut encoder, &frame);
                // Draw the shared opaque model separately, then time only the production shadow buffers.
                renderer.model.tris = None;
                renderer.model.block_order.clear();
                let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
                if let Some(q) = &query {
                    encoder.write_timestamp(q, 0);
                }
                let record_ms;
                {
                    let mut pass = encoder
                        .begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("shared model then shadow"),
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
                    body_renderer.paint(&body_frame, &mut pass, [0.0, 0.0, width as f32, height as f32]);
                    if inside && let Some(q) = &query {
                        pass.write_timestamp(q, 2);
                    }
                    let started = Instant::now();
                    renderer.paint(&frame, &mut pass, [0.0, 0.0, width as f32, height as f32]);
                    record_ms = started.elapsed().as_secs_f64() * 1000.0;
                    if inside && let Some(q) = &query {
                        pass.write_timestamp(q, 3);
                    }
                }
                if let Some(q) = &query {
                    encoder.write_timestamp(q, 1);
                    if !inside {
                        encoder.write_timestamp(q, 2);
                        encoder.write_timestamp(q, 3);
                    }
                }
                let times = finish(encoder)?;
                if sample >= warm {
                    prepare.push(prepare_ms);
                    record.push(record_ms);
                    if let Some(t) = sample_time(&times, 0, 1) {
                        full.push(t);
                    }
                    if inside && let Some(t) = sample_time(&times, 2, 3) {
                        shadow.push(t);
                    }
                }
            }
            let generations = renderer.shadow.as_ref().map_or(0, |s| s.generation);
            assert_eq!(generations, cache_generation, "cached frame regenerated shadow");
            let disable = comparison_frame(GpuScene::default(), &cam, width, height, 1001);
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.prepare(&device, &queue, &mut encoder, &disable);
            let submission = queue.submit([encoder.finish()]);
            device.poll(wgpu::PollType::Wait { submission_index: Some(submission), timeout: Some(Duration::from_secs(30)) })?;
            cached_rows.push(json!({"kind":label,"width":width,"height":height,"samples":samples,
                "full_pass_gpu_ms":summary(&mut full),"shadow_region_gpu_ms":summary(&mut shadow),
                "cpu_cached_prepare_ms":summary(&mut prepare),"cpu_shadow_record_ms":summary(&mut record),
                "projection_generations":generations,
                "cached_projection_regenerations":generations.saturating_sub(cache_generation),
                "texture_retained_after_disable":renderer.shadow.as_ref().map(|_|[SIZE,SIZE])}));
        }
    }
    if let Some(error) = pollster::block_on(scope.pop()) {
        return Err(error.to_string().into());
    }
    let result = json!({"adapter":info.name,"backend":format!("{:?}",info.backend),"timestamps":timestamps,"inside_pass":inside,
        "triangles":vertex_count/3,"updates":update_rows,"cached":cached_rows,
        "renderer_init_cpu":pipeline_rows,
        "fixture":"Two procedural boxes and a procedural torus with 64 major by 32 minor subdivisions. Earlier timings used CAD-generated meshes.",
        "notes":"Cached shadow uses the production Renderer after the shared opaque model in the same MSAA4 pass; full pass includes model and clears. Stored MSAA4 attachments prevent unused-output elimination; presentation, MSAA resolve and final downsample are excluded. 3340x1676 is SSAA2 working resolution."});
    let output = std::env::var_os("SOLIDCRAFT_BENCH_OUTPUT_DIR").map(std::path::PathBuf::from).ok_or("set SOLIDCRAFT_BENCH_OUTPUT_DIR")?;
    std::fs::create_dir_all(&output)?;
    let path = output.join("projected-shadow-comparison.json");
    std::fs::write(path, serde_json::to_vec_pretty(&result)?)?;
    println!("ellipse comparison completed: {} {:?}, inside timestamps={inside}", info.name, info.backend);
    Ok(())
}

fn summary(values: &mut [f64]) -> serde_json::Value {
    values.sort_by(f64::total_cmp);
    json!({"median":values.get(values.len()/2),"p95":values.get(values.len().saturating_mul(95)/100),"samples":values.len()})
}

/// The same torus dimensions as the original fixture, with a fixed render-only tessellation.
fn append_torus(scene: &mut GpuScene, color: super::super::PaletteIndex) -> Bounds {
    const MAJOR_SEGMENTS: u32 = 64;
    const MINOR_SEGMENTS: u32 = 32;
    const MAJOR_RADIUS: f64 = 14.0;
    const MINOR_RADIUS: f64 = 4.0;
    const CENTER_Z: f64 = 9.0;

    let vertex = |major, minor| {
        let (sin_major, cos_major) = (std::f64::consts::TAU * f64::from(major % MAJOR_SEGMENTS) / f64::from(MAJOR_SEGMENTS)).sin_cos();
        let (sin_minor, cos_minor) = (std::f64::consts::TAU * f64::from(minor % MINOR_SEGMENTS) / f64::from(MINOR_SEGMENTS)).sin_cos();
        let center = Vec3::new(MAJOR_RADIUS * cos_major, MAJOR_RADIUS * sin_major, CENTER_Z);
        let normal = Vec3::new(cos_major * cos_minor, sin_major * cos_minor, sin_minor);
        (center + normal * MINOR_RADIUS, normal)
    };
    for major in 0..MAJOR_SEGMENTS {
        for minor in 0..MINOR_SEGMENTS {
            let a = vertex(major, minor);
            let b = vertex(major + 1, minor);
            let c = vertex(major + 1, minor + 1);
            let d = vertex(major, minor + 1);
            for (position, normal) in [a, b, c, a, c, d] {
                scene.tri(position.to_f32(), normal.to_f32(), color);
            }
        }
    }
    let radius = MAJOR_RADIUS + MINOR_RADIUS;
    Bounds { min: Vec3::new(-radius, -radius, CENTER_Z - MINOR_RADIUS), max: Vec3::new(radius, radius, CENTER_Z + MINOR_RADIUS) }
}

fn append_box(scene: &mut GpuScene, bounds: Bounds, color: super::super::PaletteIndex) {
    let corners = box_corners(&bounds);
    for (normal, [a, b, c, d]) in [
        (-Vec3::Z, [0, 2, 3, 1]),
        (Vec3::Z, [4, 5, 7, 6]),
        (-Vec3::X, [0, 4, 6, 2]),
        (Vec3::X, [1, 3, 7, 5]),
        (-Vec3::Y, [0, 1, 5, 4]),
        (Vec3::Y, [2, 6, 7, 3]),
    ] {
        for index in [a, b, c, a, c, d] {
            scene.tri(corners[index].to_f32(), normal.to_f32(), color);
        }
    }
}

fn box_corners(b: &Bounds) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        Vec3::new(if i & 1 == 0 { b.min.x } else { b.max.x }, if i & 2 == 0 { b.min.y } else { b.max.y }, if i & 4 == 0 { b.min.z } else { b.max.z })
    })
}

fn bounds_in_frame(bounds: &[Bounds], cam: &Camera) -> Bounds {
    let mut combined = Bounds::EMPTY;
    for b in bounds.iter().filter(|b| !b.is_empty()) {
        for corner in box_corners(b) {
            combined.add(cam.to_frame(corner));
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
        clip: [0.0; 4],
        cap: [0.0; 4],
        images: Vec::new(),
        clouds: Vec::new(),
        analysis: [0.0; 8],
        analysis_colors: [0.0; 36],
        access: None,
        outline: false,
        eye: [0.0; 4],
        ao_radius: 0.0,
        ao_samples: crate::view::AO_SAMPLES,
        hover_tint: [0.0; 3],
        selection_tint: [0.0; 3],
        hatch: Default::default(),
        sky: [0.0; 4],
        ssaa: 1.0,
        moving: None,
    }
}

fn ellipse(sc: &mut GpuScene, b: Bounds, cam: &Camera, alpha: u8) {
    if b.is_empty() {
        return;
    }
    let dark = u32::from(alpha);
    let size = b.max - b.min;
    let c = b.center();
    let z = b.min.z - 0.001 * (1.0 + b.diagonal());
    let (rx, ry) = (size.x * 0.62 + size.z * 0.15, size.y * 0.62 + size.z * 0.15);
    if !(rx > 0.0 && ry > 0.0) {
        return;
    }
    // Rings from the middle out, fading to nothing.
    let rings = [(0.0, dark), (0.55, dark * 3 / 4), (0.85, dark / 3), (1.0, 0)];
    let n = 48;
    let at = |r: f64, k: usize| {
        let a = k as f64 / n as f64 * std::f64::consts::TAU;
        cam.from_frame(Vec3::new(c.x + rx * r * a.cos(), c.y + ry * r * a.sin(), z)).to_f32()
    };
    for w in rings.windows(2) {
        let ((r0, a0), (r1, a1)) = (w[0], w[1]);
        let (c0, c1) = (sc.color([0, 0, 0, a0 as u8]), sc.color([0, 0, 0, a1 as u8]));
        for k in 0..n {
            let (p00, p01, p10, p11) = (at(r0, k), at(r0, k + 1), at(r1, k), at(r1, k + 1));
            sc.shadow_tri(p00, c0);
            sc.shadow_tri(p10, c1);
            sc.shadow_tri(p11, c1);
            if r0 > 0.0 {
                sc.shadow_tri(p00, c0);
                sc.shadow_tri(p11, c1);
                sc.shadow_tri(p01, c0);
            }
        }
    }
}
