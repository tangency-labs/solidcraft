//! Tints from plugins for the ribbon's icons, its button blocks and the panels (capability
//! a `tint` shader, `shaders@0.2`, `docs/plugin-shaders.md`). The painters ask `ui_kit::tint` about their regions; at
//! the end of the frame the plugin's `tint` runs for all of them in one compute pass, read back
//! for a later frame. Nothing runs unless a plugin's tint is chosen on its card.

use std::sync::{Arc, Mutex, PoisonError};

use egui_wgpu::wgpu;
use solidcraft_engine::plugin::shader::{self, ShaderKind};

use crate::SolveApp;
use crate::plugin_shaders::{Loaded, Pending};
use crate::ui_kit::tint::{self, Ask};

const UNREADABLE: &str = "prefs.ui_tint_failed";

/// A tint's compute pipeline and the host's bindings (group 0).
pub(crate) struct Pipeline {
    pipeline: wgpu::ComputePipeline,
    host: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    regions_layout: wgpu::BindGroupLayout,
    buffers: Option<Buffers>,
}

/// The regions in, the tints out and a copy the CPU reads, for up to `capacity` regions.
struct Buffers {
    capacity: usize,
    regions: wgpu::Buffer,
    tints: wgpu::Buffer,
    readback: wgpu::Buffer,
    group: wgpu::BindGroup,
}

struct InFlight {
    asked: Vec<Ask>,
    mapped: Arc<Mutex<Option<bool>>>,
}

struct Active {
    key: String,
    animated: bool,
    shader: shader::Shader,
    pipeline: Pipeline,
    in_flight: Option<InFlight>,
    /// What the last pass evaluated: a still tint asked the same again needs no new pass.
    last: Option<(Vec<u8>, Vec<u8>)>,
}

#[derive(Default)]
pub(crate) struct State {
    render: Option<egui_wgpu::RenderState>,
    loading: Option<(String, Pending)>,
    active: Option<Active>,
    /// The key that failed, so it is not tried again every frame; a new choice clears it.
    failed: Option<String>,
    /// Every panel's grid of answers, one block of rows each.
    panels: Option<egui::TextureHandle>,
}

impl State {
    /// The window's GPU. A new device (after a lost one) starts over.
    pub(crate) fn install(&mut self, rs: &egui_wgpu::RenderState) {
        *self = Self { render: Some(rs.clone()), ..Self::default() };
    }
}

fn fail(app: &mut SolveApp, key: &str, error: &str) {
    app.plugin_tint.failed = Some(key.to_owned());
    app.plugin_tint.active = None;
    app.plugin_tint.loading = None;
    let message = format!("{} {key}: {error}", solidcraft_ui_text::tr(UNREADABLE));
    log::warn!("{message}");
    app.set_status(message, true);
}

/// Drops `plugin`'s tint, running or loading, so it is read again from its folder when next used.
pub(crate) fn forget(app: &mut SolveApp, plugin: &str) {
    let state = &mut app.plugin_tint;
    let owns = |key: &str| key.split_once('/').is_some_and(|(id, _)| id == plugin);
    if state.active.as_ref().is_some_and(|a| owns(&a.key)) {
        state.active = None;
    }
    if state.loading.as_ref().is_some_and(|(key, _)| owns(key)) {
        state.loading = None;
    }
}

/// The start of a frame: the chosen tint loaded and built, its last answers handed to the painters,
/// and the painters told whether to ask.
pub(crate) fn begin(app: &mut SolveApp, ctx: &egui::Context) {
    let key = app.preferences.tint_shader.clone();
    if app.plugin_tint.failed.as_ref().is_some_and(|failed| *failed != key) {
        app.plugin_tint.failed = None;
    }
    let plugin = key.split_once('/').map_or(key.as_str(), |(id, _)| id);
    let wanted = !key.is_empty()
        && app.plugin_tint.render.is_some()
        && app.plugin_tint.failed.as_deref() != Some(key.as_str())
        && crate::plugins::enabled(app, plugin);
    if !wanted || app.plugin_tint.active.as_ref().is_some_and(|a| a.key != key) {
        app.plugin_tint.active = None;
        app.plugin_tint.loading = None;
    }
    if !wanted {
        tint::set_on(ctx, false);
        return;
    }
    let Some(rs) = app.plugin_tint.render.clone() else { return };
    if rs.device_lost.is_lost() {
        // The tint may be why the driver gave up: off until it is chosen again.
        app.preferences.tint_shader.clear();
        fail(app, &key, "graphics device lost");
        tint::set_on(ctx, false);
        return;
    }
    if app.plugin_tint.active.is_none() {
        tint::set_on(ctx, false);
        if app.plugin_tint.loading.as_ref().is_none_or(|(k, _)| *k != key) {
            let Some(folder) = crate::plugin_shaders::folder_of(app, &key) else {
                fail(app, &key, solidcraft_ui_text::tr("prefs.background_shader_missing"));
                return;
            };
            let pending: Pending = Arc::default();
            let (slot, repaint, k, limit) = (pending.clone(), ctx.clone(), key.clone(), rs.device.limits().max_texture_dimension_2d);
            std::thread::spawn(move || {
                let result = crate::plugin_shaders::load(folder, k, limit);
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
                repaint.request_repaint();
            });
            app.plugin_tint.loading = Some((key, pending));
            return;
        }
        let done = app.plugin_tint.loading.as_ref().and_then(|(_, p)| p.lock().unwrap_or_else(PoisonError::into_inner).take());
        let Some(done) = done else { return };
        app.plugin_tint.loading = None;
        match done.and_then(|loaded| build(&rs.adapter, &rs.device, &rs.queue, loaded)) {
            Ok((pipeline, loaded)) => {
                app.plugin_tint.active =
                    Some(Active { key, animated: loaded.animated, shader: loaded.shader, pipeline, in_flight: None, last: None });
            }
            Err(error) => {
                fail(app, &key, &error);
                return;
            }
        }
    }
    let Some(active) = app.plugin_tint.active.as_mut() else { return };
    tint::set_on(ctx, true);
    if let Some(answers) = collect(&rs.device, active) {
        tint::answer(ctx, &answers.0, &answers.1);
    }
    if active.animated {
        let focused = ctx.input(|i| i.focused);
        ctx.request_repaint_after(std::time::Duration::from_millis(if focused { 33 } else { 100 }));
    }
}

/// The end of a frame: the panels tinted, then one pass over every region asked about.
pub(crate) fn end(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(rs) = app.plugin_tint.render.clone() else { return };
    let Some(active) = app.plugin_tint.active.as_ref() else { return };
    let t = solidcraft_theme::Tokens::get();
    let settings = crate::plugin_shaders::settings(&app.preferences.shader_settings, &active.key, &active.shader);
    let mut texture = app.plugin_tint.panels.take();
    panels(ctx, &crate::ui_surfaces::fills(&t), &mut texture);
    app.plugin_tint.panels = texture;
    let Some(active) = app.plugin_tint.active.as_mut() else { return };
    let asked = tint::take_asked(ctx);
    if asked.is_empty() {
        return;
    }
    if active.in_flight.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
        return;
    }
    let time = if active.animated { (ctx.input(|i| i.time) % 3600.0) as f32 } else { 0.0 };
    let uniforms = host_uniforms(ctx, time, &t, settings);
    let regions: Vec<u8> = asked.iter().take(shader::MAX_TINT_REGIONS).flat_map(|a| a.bytes()).collect();
    if active.last.as_ref().is_some_and(|(r, u)| *r == regions && *u == uniforms) {
        return;
    }
    let asked: Vec<Ask> = asked.into_iter().take(shader::MAX_TINT_REGIONS).collect();
    let mapped = dispatch(&rs.device, &rs.queue, &mut active.pipeline, &regions, &uniforms);
    active.in_flight = Some(InFlight { asked, mapped });
    active.last = Some((regions, uniforms));
    ctx.request_repaint_after(std::time::Duration::from_millis(16));
}

/// The largest step between the points asked about on a panel, in points.
const PANEL_STEP: f32 = 48.0;
/// The most points asked about across or down one panel.
const PANEL_POINTS: usize = 24;

/// The points asked about on a panel: a grid at most [`PANEL_STEP`] apart, corners included.
fn panel_grid(rect: egui::Rect) -> (usize, usize) {
    let n = |len: f32| ((len / PANEL_STEP).ceil() as usize).clamp(1, PANEL_POINTS - 1) + 1;
    (n(rect.width()), n(rect.height()))
}

/// The tint over each panel this frame, as a gradient through the points asked about, inside the
/// panel's corners. One texture holds every panel's grid; bilinear filtering makes the gradient.
pub(crate) fn panels(ctx: &egui::Context, fills: &[egui::Color32], texture: &mut Option<egui::TextureHandle>) {
    use egui::epaint::{RectShape, Shape};
    if !tint::is_on(ctx) {
        return;
    }
    let found = crate::ui_surfaces::scan(ctx, fills);
    if found.is_empty() {
        return;
    }
    let width = found.iter().map(|s| panel_grid(s.shape.rect).0).max().unwrap_or(1);
    let height: usize = found.iter().map(|s| panel_grid(s.shape.rect).1).sum();
    let mut pixels = vec![egui::Color32::TRANSPARENT; width * height];
    let mut overlays = Vec::with_capacity(found.len());
    let (mut row, mut any) = (0, false);
    for surface in &found {
        let rect = surface.shape.rect;
        let (cols, rows) = panel_grid(rect);
        for y in 0..rows {
            for x in 0..cols {
                let at = egui::pos2(
                    egui::lerp(rect.left()..=rect.right(), x as f32 / (cols - 1) as f32),
                    egui::lerp(rect.top()..=rect.bottom(), y as f32 / (rows - 1) as f32),
                );
                let asked = Ask { region: tint::Region::Panel, rect, at, base: surface.shape.fill, enabled: 1.0 };
                if let Some(answer) = tint::ask(ctx, asked) {
                    pixels[(row + y) * width + x] = tint::colour(answer);
                    any |= answer[3] > 0.0;
                }
            }
        }
        let texel = |i: usize, n: usize| (i as f32 + 0.5) / n as f32;
        let uv = egui::Rect::from_min_max(
            egui::pos2(texel(0, width), texel(row, height)),
            egui::pos2(texel(cols - 1, width), texel(row + rows - 1, height)),
        );
        overlays.push((surface, uv));
        row += rows;
    }
    if !any {
        return;
    }
    let image = egui::ColorImage { size: [width, height], source_size: egui::vec2(width as f32, height as f32), pixels };
    match texture {
        Some(handle) => handle.set(image, egui::TextureOptions::LINEAR),
        None => *texture = Some(ctx.load_texture("sc_plugin_tint_panels", image, egui::TextureOptions::LINEAR)),
    }
    let Some(id) = texture.as_ref().map(egui::TextureHandle::id) else { return };
    for (surface, uv) in overlays {
        let over = RectShape::filled(surface.shape.rect, surface.shape.corner_radius, egui::Color32::WHITE).with_texture(id, uv);
        crate::ui_surfaces::add_over(ctx, surface, fills, Shape::Rect(over));
    }
}

/// The answers of the pass in flight, once the GPU has them.
fn collect(device: &wgpu::Device, active: &mut Active) -> Option<(Vec<Ask>, Vec<[f32; 4]>)> {
    let flight = active.in_flight.as_ref()?;
    let _ = device.poll(wgpu::PollType::Poll);
    let done = *flight.mapped.lock().unwrap_or_else(PoisonError::into_inner);
    let ok = done?;
    let flight = active.in_flight.take()?;
    let buffers = active.pipeline.buffers.as_ref()?;
    if !ok {
        return None;
    }
    let tints = read(&buffers.readback, flight.asked.len());
    Some((flight.asked, tints))
}

fn read(readback: &wgpu::Buffer, count: usize) -> Vec<[f32; 4]> {
    let tints = {
        let Ok(view) = readback.slice(..(count * 16) as u64).get_mapped_range() else {
            readback.unmap();
            return Vec::new();
        };
        view.as_chunks::<16>()
            .0
            .iter()
            .map(|c| {
                let f = |i: usize| f32::from_le_bytes([c[i * 4], c[i * 4 + 1], c[i * 4 + 2], c[i * 4 + 3]]);
                [f(0), f(1), f(2), f(3)]
            })
            .collect()
    };
    readback.unmap();
    tints
}

/// `ScHost` for a tint: the window's size in pixels, no camera.
fn host_uniforms(ctx: &egui::Context, time: f32, t: &solidcraft_theme::Tokens, settings: [f32; shader::MAX_SETTINGS]) -> Vec<u8> {
    let ppp = ctx.pixels_per_point();
    let size = ctx.content_rect().size() * ppp;
    let dark = egui::Rgba::from(t.panel).intensity() < 0.25;
    let mut f: Vec<f32> = vec![size.x, size.y, time, ppp];
    f.extend([0.0; 12]);
    for c in [t.viewport_top, t.viewport_bottom, t.accent, t.text] {
        f.extend(crate::plugin_shaders::linear(c));
    }
    f.extend([if dark { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0]);
    f.extend(settings);
    debug_assert_eq!(f.len() * 4, shader::HOST_UNIFORM_BYTES);
    f.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The compute pipeline for `loaded`, made under error scopes: a module this graphics card refuses
/// is an error here, never a crash.
pub(crate) fn build(adapter: &wgpu::Adapter, device: &wgpu::Device, queue: &wgpu::Queue, loaded: Loaded) -> Result<(Pipeline, Loaded), String> {
    if loaded.kind != ShaderKind::Tint {
        return Err("not a tint".into());
    }
    if !adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS) {
        return Err("this graphics card runs no compute shaders".into());
    }
    let scopes = [wgpu::ErrorFilter::Validation, wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal].map(|f| device.push_error_scope(f));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sc_plugin_tint"),
        source: wgpu::ShaderSource::Wgsl(loaded.wgsl.as_str().into()),
    });
    let entry =
        |binding: u32, ty: wgpu::BindingType| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty, count: None };
    let storage = |read_only: bool| wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only },
        has_dynamic_offset: false,
        min_binding_size: None,
    };
    let mut entries = vec![
        entry(0, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }),
        entry(1, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
        entry(2, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
    ];
    for i in 0..loaded.textures.len() {
        entries.push(entry(
            shader::FIRST_TEXTURE_BINDING + i as u32,
            wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
        ));
    }
    let host_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sc_plugin_tint_host"), entries: &entries });
    let regions_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sc_plugin_tint_regions"),
        entries: &[entry(0, storage(true)), entry(1, storage(false))],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sc_plugin_tint"),
        bind_group_layouts: &[Some(&host_layout), Some(&regions_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("sc_plugin_tint"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("sc_tint_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sc_plugin_tint"),
        size: shader::HOST_UNIFORM_BYTES as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sampler = |mode: wgpu::AddressMode| {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_plugin_tint"),
            address_mode_u: mode,
            address_mode_v: mode,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        })
    };
    let (clamp, repeat) = (sampler(wgpu::AddressMode::ClampToEdge), sampler(wgpu::AddressMode::Repeat));
    let views: Vec<wgpu::TextureView> = loaded.textures.iter().map(|levels| crate::plugin_shaders::upload(device, queue, levels)).collect();
    let mut bindings = vec![
        wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&clamp) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&repeat) },
    ];
    for (i, view) in views.iter().enumerate() {
        bindings.push(wgpu::BindGroupEntry { binding: shader::FIRST_TEXTURE_BINDING + i as u32, resource: wgpu::BindingResource::TextureView(view) });
    }
    let host = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sc_plugin_tint_host"), layout: &host_layout, entries: &bindings });
    let mut error = None;
    for scope in scopes.into_iter().rev() {
        if let Some(e) = pollster::block_on(scope.pop()) {
            error = Some(e.to_string());
        }
    }
    if let Some(error) = error {
        return Err(error);
    }
    drop(views);
    Ok((Pipeline { pipeline, host, uniforms, regions_layout, buffers: None }, Loaded { textures: Vec::new(), ..loaded }))
}

/// Runs the tint over `regions` (`ScRegion`s, as `Ask::bytes` writes them) and starts reading the
/// answers back; the flag is set once they can be read (true) or could not be (false).
pub(crate) fn dispatch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &mut Pipeline,
    regions: &[u8],
    uniforms: &[u8],
) -> Arc<Mutex<Option<bool>>> {
    let count = regions.len() / shader::TINT_REGION_BYTES;
    if pipeline.buffers.as_ref().is_none_or(|b| b.capacity < count) {
        let capacity = count.next_power_of_two().max(64);
        let buffer = |label: &str, size: usize, usage: wgpu::BufferUsages| {
            device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage, mapped_at_creation: false })
        };
        let regions =
            buffer("sc_plugin_tint_regions", capacity * shader::TINT_REGION_BYTES, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST);
        let tints = buffer("sc_plugin_tint_out", capacity * 16, wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let readback = buffer("sc_plugin_tint_read", capacity * 16, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_plugin_tint_regions"),
            layout: &pipeline.regions_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: regions.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tints.as_entire_binding() },
            ],
        });
        pipeline.buffers = Some(Buffers { capacity, regions, tints, readback, group });
    }
    let mapped: Arc<Mutex<Option<bool>>> = Arc::default();
    let Some(buffers) = pipeline.buffers.as_ref() else { return mapped };
    queue.write_buffer(&buffers.regions, 0, regions);
    queue.write_buffer(&pipeline.uniforms, 0, uniforms);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_plugin_tint") });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sc_plugin_tint"), timestamp_writes: None });
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &pipeline.host, &[]);
        pass.set_bind_group(1, &buffers.group, &[]);
        pass.dispatch_workgroups((count as u32).div_ceil(shader::TINT_WORKGROUP), 1, 1);
    }
    let bytes = (count * 16) as u64;
    encoder.copy_buffer_to_buffer(&buffers.tints, 0, &buffers.readback, 0, bytes);
    queue.submit([encoder.finish()]);
    let flag = mapped.clone();
    buffers.readback.slice(..bytes).map_async(wgpu::MapMode::Read, move |result| {
        *flag.lock().unwrap_or_else(PoisonError::into_inner) = Some(result.is_ok());
    });
    mapped
}

/// Runs a tint once and waits for its answers: for tests and checks off the window.
#[cfg(test)]
pub(crate) fn evaluate(
    adapter: &wgpu::Adapter,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    loaded: Loaded,
    asked: &[Ask],
    time: f32,
) -> Result<Vec<[f32; 4]>, String> {
    let settings = loaded.shader.setting_values(|_| None);
    let (mut pipeline, _) = build(adapter, device, queue, loaded)?;
    let regions: Vec<u8> = asked.iter().flat_map(|a| a.bytes()).collect();
    let mut uniforms = vec![0u8; shader::HOST_UNIFORM_BYTES];
    uniforms[0..4].copy_from_slice(&1600f32.to_le_bytes());
    uniforms[4..8].copy_from_slice(&900f32.to_le_bytes());
    uniforms[8..12].copy_from_slice(&time.to_le_bytes());
    uniforms[12..16].copy_from_slice(&1f32.to_le_bytes());
    for (i, v) in settings.iter().enumerate() {
        uniforms[144 + i * 4..148 + i * 4].copy_from_slice(&v.to_le_bytes());
    }
    let mapped = dispatch(device, queue, &mut pipeline, &regions, &uniforms);
    device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_secs(30)) }).map_err(|e| e.to_string())?;
    if *mapped.lock().unwrap_or_else(PoisonError::into_inner) != Some(true) {
        return Err("the answers could not be read back".into());
    }
    let buffers = pipeline.buffers.as_ref().ok_or("no buffers")?;
    Ok(read(&buffers.readback, asked.len()))
}

#[cfg(test)]
#[path = "plugin_tint_tests.rs"]
mod tests;
