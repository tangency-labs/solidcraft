//! View backgrounds from plugins (capability `shaders@0.1`, `docs/plugin-shaders.md`).
//!
//! The WGSL is checked off the window thread and built under an error scope on it; whatever fails
//! leaves the theme's background, with one line in the status bar.

use std::sync::{Arc, Mutex, PoisonError};

use egui_wgpu::wgpu;

use solidcraft_engine::plugin::shader::STATE_BINDING;

use crate::SolveApp;

/// The status line's message when a background can't be drawn.
const UNREADABLE: &str = "prefs.background_shader_failed";

struct Loaded {
    wgsl: String,
    animated: bool,
    /// Each texture's mip levels, largest first: size and straight RGBA.
    textures: Vec<Vec<([u32; 2], Vec<u8>)>>,
    state: Option<[u32; 2]>,
}

type Pending = Arc<Mutex<Option<Result<Loaded, String>>>>;

/// A background ready to draw, and its target the size of the view.
struct Active {
    key: String,
    animated: bool,
    pipeline: wgpu::RenderPipeline,
    /// One, or two for a state shader: each binds one grid to both state bindings, read as prev or current.
    bind_groups: Vec<wgpu::BindGroup>,
    uniforms: wgpu::Buffer,
    target: Option<Target>,
    state: Option<Sim>,
}

/// A state shader's two grids: each frame the step writes the other one, then the background reads it.
struct Sim {
    pipeline: wgpu::RenderPipeline,
    views: [wgpu::TextureView; 2],
    size: [u32; 2],
    /// The grid the last step wrote.
    current: usize,
    /// The step this frame: 0 on the first, then counting up (back to 1 after `FRAME_WRAP`).
    frame: u32,
}

/// The step counter's wrap: well past the grid's hash and warm-up needs, and far from f32's exact range.
const FRAME_WRAP: u32 = 1_000_000;

/// The next step's number: counting up, and back to 1 (never 0, which is the reset) after `FRAME_WRAP`.
fn next_step(frame: u32) -> u32 {
    if frame >= FRAME_WRAP { 1 } else { frame + 1 }
}

struct Target {
    size: [u32; 2],
    /// The uniforms of the last frame drawn into it: a still background draws again only when
    /// they change (the view turned, resized or the theme changed).
    drawn: Option<Vec<u8>>,
    view: wgpu::TextureView,
    id: egui::TextureId,
}

#[derive(Default)]
pub(crate) struct State {
    render: Option<egui_wgpu::RenderState>,
    loading: Option<(String, Pending)>,
    active: Option<Active>,
    /// The key that failed, so it is not tried again every frame; a new choice clears it.
    failed: Option<String>,
    /// `failed` is only because its plugin is turned off: turning it on again clears it.
    off: bool,
}

impl State {
    /// The window's GPU. A new device (after a lost one) starts over.
    pub(crate) fn install(&mut self, rs: &egui_wgpu::RenderState) {
        *self = Self { render: Some(rs.clone()), ..Self::default() };
    }
}

/// The folder of the turned-on plugin that offers background `key`, if one is installed.
fn folder_of(app: &SolveApp, key: &str) -> Option<String> {
    let (plugin, shader) = key.split_once('/')?;
    if !crate::plugins::enabled(app, plugin) {
        return None;
    }
    let mut paths: Vec<String> = app.session.plugin_state.packages.get(plugin).cloned().into_iter().collect();
    if let Some(dir) = app.session.plugin_state.plugins_dir() {
        paths.push(dir.join(plugin).to_string_lossy().into_owned());
    }
    paths.into_iter().find(|path| manifest(path).is_ok_and(|m| m.id == plugin && m.shaders.iter().any(|s| s.id == shader)))
}

/// A plugin folder's manifest alone: a background needs no component.
fn manifest(folder: &str) -> Result<solidcraft_engine::plugin::Manifest, String> {
    let dir = std::path::Path::new(folder);
    let name = dir.file_name().and_then(|n| n.to_str()).ok_or("a plugin folder needs a name")?;
    let path = dir.join("plugin.json");
    let size = std::fs::metadata(&path).map_err(|e| format!("plugin.json: {e}"))?.len();
    if size > solidcraft_engine::plugin::MAX_MANIFEST_BYTES as u64 {
        return Err("plugin.json: too large".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("plugin.json: {e}"))?;
    solidcraft_engine::plugin::parse_manifest(&bytes, name).map_err(|e| e.to_string())
}

/// Reads and checks the chosen background off the window thread.
fn load(folder: String, key: String, limit: u32) -> Result<Loaded, String> {
    let manifest = manifest(&folder)?;
    let shader_id = key.split_once('/').map(|(_, s)| s).unwrap_or_default();
    let shader = manifest.shaders.iter().find(|s| s.id == shader_id).ok_or_else(|| format!("{key} is not in the plugin any more"))?;
    let dir = std::path::Path::new(&folder);
    let read = |file: &str, max: usize| -> Result<Vec<u8>, String> {
        let path = dir.join(file);
        let size = std::fs::metadata(&path).map_err(|e| format!("{file}: {e}"))?.len();
        if size > max as u64 {
            return Err(format!("{file}: larger than {} KiB", max / 1024));
        }
        std::fs::read(&path).map_err(|e| format!("{file}: {e}"))
    };
    let source = read(&shader.source, solidcraft_engine::plugin::shader::MAX_SOURCE_BYTES)?;
    let source = String::from_utf8(source).map_err(|_| format!("{} is not UTF-8 text", shader.source))?;
    let compiled = solidcraft_engine::plugin::shader::compile(shader, &source).map_err(|e| e.to_string())?;
    let side = limit.min(solidcraft_engine::plugin::shader::MAX_TEXTURE_SIDE);
    let textures = shader
        .textures
        .iter()
        .map(|t| {
            read(&t.file, solidcraft_engine::plugin::shader::MAX_TEXTURE_BYTES)
                .and_then(|bytes| mips(&bytes, side).map_err(|e| format!("{}: {e}", t.file)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Loaded { wgsl: compiled.wgsl, animated: shader.animated, textures, state: shader.state.map(|s| [s.width, s.height]) })
}

/// A picture's mip levels, at most `limit` on a side, down to 1×1.
fn mips(bytes: &[u8], limit: u32) -> Result<Vec<([u32; 2], Vec<u8>)>, String> {
    let mut image = image::load_from_memory(bytes).map_err(|e| e.to_string())?.into_rgba8();
    if image.width().max(image.height()) > limit {
        let (w, h) = (image.width(), image.height());
        let scale = f64::from(limit) / f64::from(w.max(h));
        let fit = |v: u32| ((f64::from(v) * scale).round() as u32).max(1);
        image = image::imageops::resize(&image, fit(w).min(w), fit(h).min(h), image::imageops::FilterType::Triangle);
    }
    let mut levels = Vec::new();
    loop {
        let (w, h) = (image.width(), image.height());
        let next = (w > 1 || h > 1).then(|| image::imageops::resize(&image, (w / 2).max(1), (h / 2).max(1), image::imageops::FilterType::Triangle));
        levels.push(([w, h], image.into_raw()));
        match next {
            Some(smaller) => image = smaller,
            None => return Ok(levels),
        }
    }
}

pub(crate) fn paint(app: &mut SolveApp, painter: &egui::Painter, rect: egui::Rect, tokens: &solidcraft_theme::Tokens) {
    if !allowed(app) {
        return;
    }
    let key = app.preferences.background_shader.clone();
    let state = &mut app.plugin_shaders;
    if state.failed.as_ref().is_some_and(|failed| *failed != key) {
        // Another choice: the one that failed may be tried again when it is chosen again.
        state.failed = None;
    }
    if key.is_empty() || state.render.is_none() || state.failed.as_deref() == Some(key.as_str()) {
        return;
    }
    let ctx = painter.ctx().clone();
    if state.active.as_ref().is_some_and(|a| a.key != key) {
        retire(state);
    }
    if state.active.is_none() {
        let Some(rs) = state.render.clone() else { return };
        if state.loading.as_ref().is_none_or(|(k, _)| *k != key) {
            let Some(folder) = folder_of(app, &key) else {
                missing(app, &key);
                return;
            };
            let pending: Pending = Arc::default();
            let (slot, repaint, k, limit) = (pending.clone(), ctx.clone(), key.clone(), rs.device.limits().max_texture_dimension_2d);
            std::thread::spawn(move || {
                let result = load(folder, k, limit);
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
                repaint.request_repaint();
            });
            app.plugin_shaders.loading = Some((key.clone(), pending));
            return;
        }
        let state = &mut app.plugin_shaders;
        let done = state.loading.as_ref().and_then(|(_, p)| p.lock().unwrap_or_else(PoisonError::into_inner).take());
        let Some(done) = done else { return };
        state.loading = None;
        match done.and_then(|loaded| build(&rs, &key, loaded)) {
            Ok(active) => state.active = Some(active),
            Err(error) => {
                state.failed = Some(key.clone());
                let message = format!("{} {key}: {error}", solidcraft_ui_text::tr(UNREADABLE));
                log::warn!("{message}");
                app.set_status(message, true);
                return;
            }
        }
    }
    let state = &mut app.plugin_shaders;
    let (Some(rs), Some(active)) = (state.render.clone(), state.active.as_mut()) else { return };
    if rs.device_lost.is_lost() {
        // The shader may be why the driver gave up: turn it off before the app restarts, so the
        // next start does not run it again (the restart also turns it off, `after_loss`).
        let key = active.key.clone();
        app.plugin_shaders.failed = Some(key.clone());
        app.preferences.background_shader.clear();
        log::warn!("graphics device lost while the background effect {key} was drawn; it is turned off");
        return;
    }
    let ppp = ctx.pixels_per_point();
    let max = rs.device.limits().max_texture_dimension_2d;
    let size = [((rect.width() * ppp).round() as u32).clamp(1, max), ((rect.height() * ppp).round() as u32).clamp(1, max)];
    let time = if active.animated {
        let focused = ctx.input(|i| i.focused);
        // Slower behind another window, where nobody is working.
        ctx.request_repaint_after(std::time::Duration::from_millis(if focused { 33 } else { 100 }));
        (ctx.input(|i| i.time) % 3600.0) as f32
    } else {
        0.0
    };
    let (step, grid) = match active.state.as_mut() {
        Some(sim) => {
            let step = sim.frame;
            sim.frame = next_step(step);
            (step as f32, sim.size.map(|v| v as f32))
        }
        None => (0.0, [0.0, 0.0]),
    };
    let uniforms = host_uniforms(size, time, ppp, &app.cam, tokens, step, grid);
    let Some(id) = draw(&rs, active, size, &uniforms) else { return };
    painter.image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
}

/// A plugin turned off after its background was drawn, or while it loads, drops it, saying so once.
fn allowed(app: &mut SolveApp) -> bool {
    let key = app.preferences.background_shader.clone();
    if key.is_empty() {
        return true;
    }
    let plugin = key.split_once('/').map_or(key.as_str(), |(id, _)| id);
    let enabled = crate::plugins::enabled(app, plugin);
    let state = &mut app.plugin_shaders;
    if state.failed.as_deref() == Some(key.as_str()) {
        if !(state.off && enabled) {
            return true;
        }
        // Its plugin is on again: try the background again.
        state.failed = None;
    }
    state.off = false;
    if enabled {
        return true;
    }
    forget(app, plugin);
    missing(app, &key);
    app.plugin_shaders.off = true;
    false
}

/// Drops `plugin`'s background, drawn or loading, so it is read again from its folder when next chosen.
pub(crate) fn forget(app: &mut SolveApp, plugin: &str) {
    let state = &mut app.plugin_shaders;
    let owns = |key: &str| key.split_once('/').is_some_and(|(id, _)| id == plugin);
    if state.active.as_ref().is_some_and(|a| owns(&a.key)) {
        retire(state);
    }
    if state.loading.as_ref().is_some_and(|(key, _)| owns(key)) {
        state.loading = None;
    }
}

/// The chosen background's plugin is missing or off: say so once, not every frame.
fn missing(app: &mut SolveApp, key: &str) {
    app.plugin_shaders.failed = Some(key.to_owned());
    let message = format!("{} {key}: {}", solidcraft_ui_text::tr(UNREADABLE), solidcraft_ui_text::tr("prefs.background_shader_missing"));
    log::warn!("{message}");
    app.set_status(message, true);
}

fn retire(state: &mut State) {
    if let (Some(rs), Some(active)) = (&state.render, state.active.take())
        && let Some(target) = active.target
    {
        rs.renderer.write().free_texture(&target.id);
    }
}

/// Linear-light RGBA of a theme colour, alpha 1.
fn linear(c: egui::Color32) -> [f32; 4] {
    let l = egui::Rgba::from(c);
    [l.r(), l.g(), l.b(), 1.0]
}

/// `ScHost` as the shader reads it (`solidcraft_engine::plugin::shader::PRELUDE`).
fn host_uniforms(
    size: [u32; 2],
    time: f32,
    scale: f32,
    cam: &solidcraft_viewport::Camera,
    t: &solidcraft_theme::Tokens,
    step: f32,
    grid: [f32; 2],
) -> Vec<u8> {
    const HALF_HEIGHT: f64 = 0.5;
    let (right, up, back) = cam.basis();
    let aspect = f64::from(size[0]) / f64::from(size[1].max(1));
    let axis = |v: solidcore_record::Vec3, by: f64| [(v.x * by) as f32, (v.y * by) as f32, (v.z * by) as f32, 0.0];
    let dark = egui::Rgba::from(t.viewport_bottom).intensity() < 0.25;
    let mut f: Vec<f32> = vec![size[0] as f32, size[1] as f32, time, scale];
    f.extend(axis(right, HALF_HEIGHT * aspect));
    f.extend(axis(up, HALF_HEIGHT));
    f.extend(axis(back, -1.0));
    for c in [t.viewport_top, t.viewport_bottom, t.accent, t.text] {
        f.extend(linear(c));
    }
    f.extend([if dark { 1.0 } else { 0.0 }, step, grid[0], grid[1]]);
    debug_assert_eq!(f.len() * 4, solidcraft_engine::plugin::shader::HOST_UNIFORM_BYTES);
    f.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Plain, not sRGB: the view shows a native texel as a screen value and blends there, so the
/// fragment stage stores the colour sRGB-encoded and premultiplied.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Half floats, so a state keeps its precision between frames.
const FORMAT_STATE: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The pipeline and bindings for `loaded`, made under error scopes: a module this graphics card
/// refuses is an error here, never a crash.
fn build(rs: &egui_wgpu::RenderState, key: &str, loaded: Loaded) -> Result<Active, String> {
    let device = &rs.device;
    let scopes = [wgpu::ErrorFilter::Validation, wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal].map(|f| device.push_error_scope(f));
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sc_plugin_background"),
        source: wgpu::ShaderSource::Wgsl(loaded.wgsl.as_str().into()),
    });
    let mut entries = vec![
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
    ];
    for i in 0..loaded.textures.len() {
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: solidcraft_engine::plugin::shader::FIRST_TEXTURE_BINDING + i as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    // The state grids are read with textureLoad only, so they need not be filterable.
    if loaded.state.is_some() {
        for binding in [STATE_BINDING, STATE_BINDING + 1] {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
    }
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sc_plugin_background"), entries: &entries });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sc_plugin_background"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = make_pipeline(device, &module, &pipeline_layout, "sc_fragment", FORMAT);
    let step = loaded.state.map(|_| make_pipeline(device, &module, &pipeline_layout, "sc_state_fragment", FORMAT_STATE));
    let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sc_plugin_background"),
        size: solidcraft_engine::plugin::shader::HOST_UNIFORM_BYTES as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sampler = |mode: wgpu::AddressMode| {
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_plugin_background"),
            address_mode_u: mode,
            address_mode_v: mode,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        })
    };
    let (clamp, repeat) = (sampler(wgpu::AddressMode::ClampToEdge), sampler(wgpu::AddressMode::Repeat));
    let views: Vec<wgpu::TextureView> = loaded.textures.iter().map(|levels| upload(rs, levels)).collect();
    let samplers = (&clamp, &repeat);
    let grids: Vec<wgpu::TextureView> = loaded.state.map_or_else(Vec::new, |size| (0..2).map(|_| grid(device, size)).collect());
    let bind_groups: Vec<wgpu::BindGroup> = if grids.is_empty() {
        vec![group(device, &layout, &uniforms, samplers, &views, None)]
    } else {
        grids.iter().map(|grid| group(device, &layout, &uniforms, samplers, &views, Some(grid))).collect()
    };
    let mut error = None;
    for scope in scopes.into_iter().rev() {
        if let Some(e) = pollster::block_on(scope.pop()) {
            error = Some(e.to_string());
        }
    }
    if let Some(error) = error {
        return Err(error);
    }
    let state = match (loaded.state, step) {
        (Some(size), Some(pipeline)) => {
            <[wgpu::TextureView; 2]>::try_from(grids).ok().map(|views| Sim { pipeline, views, size, current: 0, frame: 0 })
        }
        _ => None,
    };
    Ok(Active { key: key.to_string(), animated: loaded.animated, pipeline, bind_groups, uniforms, target: None, state })
}

fn make_pipeline(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    entry: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sc_plugin_background"),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("sc_vertex"), compilation_options: Default::default(), buffers: &[] },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// One of a state shader's grids, zeroed by the GPU.
fn grid(device: &wgpu::Device, size: [u32; 2]) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc_plugin_background_state"),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT_STATE,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// With a `grid`, both state bindings name it: the step reads it as `sc_state_prev`, the background as `sc_state`.
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniforms: &wgpu::Buffer,
    (clamp, repeat): (&wgpu::Sampler, &wgpu::Sampler),
    views: &[wgpu::TextureView],
    grid: Option<&wgpu::TextureView>,
) -> wgpu::BindGroup {
    let mut bindings = vec![
        wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(clamp) },
        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(repeat) },
    ];
    for (i, view) in views.iter().enumerate() {
        bindings.push(wgpu::BindGroupEntry {
            binding: solidcraft_engine::plugin::shader::FIRST_TEXTURE_BINDING + i as u32,
            resource: wgpu::BindingResource::TextureView(view),
        });
    }
    if let Some(grid) = grid {
        for binding in [STATE_BINDING, STATE_BINDING + 1] {
            bindings.push(wgpu::BindGroupEntry { binding, resource: wgpu::BindingResource::TextureView(grid) });
        }
    }
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("sc_plugin_background"), layout, entries: &bindings })
}

fn upload(rs: &egui_wgpu::RenderState, levels: &[([u32; 2], Vec<u8>)]) -> wgpu::TextureView {
    let [w, h] = levels.first().map_or([1, 1], |(size, _)| *size);
    let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc_plugin_background_picture"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels.len().max(1) as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, ([lw, lh], rgba)) in levels.iter().enumerate() {
        rs.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * lw), rows_per_image: Some(*lh) },
            wgpu::Extent3d { width: *lw, height: *lh, depth_or_array_layers: 1 },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn draw(rs: &egui_wgpu::RenderState, active: &mut Active, size: [u32; 2], uniforms: &[u8]) -> Option<egui::TextureId> {
    if active.target.as_ref().is_none_or(|t| t.size != size) {
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sc_plugin_background_target"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = rs.renderer.write();
        let id = match active.target.take() {
            Some(old) => {
                renderer.update_egui_texture_from_wgpu_texture(&rs.device, &view, wgpu::FilterMode::Linear, old.id);
                old.id
            }
            None => renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear),
        };
        active.target = Some(Target { size, view, id, drawn: None });
    }
    let target = active.target.as_mut()?;
    if target.drawn.as_deref() == Some(uniforms) {
        return Some(target.id);
    }
    target.drawn = Some(uniforms.to_vec());
    rs.queue.write_buffer(&active.uniforms, 0, uniforms);
    let mut encoder = rs.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_plugin_background") });
    // The state step first, writing the grid that is not current; the background then reads it.
    let mut group = 0;
    if let Some(sim) = active.state.as_mut() {
        let prev = sim.current;
        let next = 1 - prev;
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sc_plugin_background_step"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &sim.views[next],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&sim.pipeline);
            pass.set_bind_group(0, &active.bind_groups[prev], &[]);
            pass.draw(0..3, 0..1);
        }
        sim.current = next;
        group = next;
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_plugin_background"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&active.pipeline);
        pass.set_bind_group(0, &active.bind_groups[group], &[]);
        pass.draw(0..3, 0..1);
    }
    rs.queue.submit([encoder.finish()]);
    Some(target.id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_picture_gets_every_mip_level_down_to_one_pixel_and_fits_the_limit() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(40, 20).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let levels = super::mips(png.get_ref(), 16).unwrap();
        let sizes: Vec<[u32; 2]> = levels.iter().map(|(s, _)| *s).collect();
        assert_eq!(sizes, vec![[16, 8], [8, 4], [4, 2], [2, 1], [1, 1]]);
        assert!(levels.iter().all(|([w, h], rgba)| rgba.len() == (w * h * 4) as usize));
        assert!(super::mips(b"not a picture", 16).is_err());
    }

    #[test]
    fn the_host_uniforms_are_the_size_the_prelude_declares() {
        let tokens = solidcraft_theme::Tokens::get();
        let bytes = super::host_uniforms([800, 400], 1.5, 2.0, &solidcraft_viewport::Camera::default(), &tokens, 7.0, [96.0, 48.0]);
        assert_eq!(bytes.len(), solidcraft_engine::plugin::shader::HOST_UNIFORM_BYTES);
        let first: Vec<f32> = bytes.chunks(4).take(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        assert_eq!(first, vec![800.0, 400.0, 1.5, 2.0]);
        // The step and the grid's size follow the theme's dark flag, at the prelude's offsets.
        let floats: Vec<f32> = bytes.chunks(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        assert_eq!(&floats[33..36], &[7.0, 96.0, 48.0]);
    }

    #[test]
    fn the_step_counts_up_and_never_wraps_to_the_reset_value() {
        assert_eq!(super::next_step(0), 1);
        assert_eq!(super::next_step(super::FRAME_WRAP - 1), super::FRAME_WRAP);
        assert_eq!(super::next_step(super::FRAME_WRAP), 1);
    }

    #[test]
    fn the_background_turns_with_the_camera_but_not_with_zoom_or_pan() {
        let tokens = solidcraft_theme::Tokens::get();
        let at = |cam: &solidcraft_viewport::Camera| super::host_uniforms([800, 400], 0.0, 1.0, cam, &tokens, 0.0, [0.0, 0.0]);
        let cam = solidcraft_viewport::Camera::default();
        let mut turned = cam;
        turned.orbit(200.0, 0.0);
        assert_ne!(at(&cam), at(&turned));
        let mut moved = cam;
        moved.distance *= 3.0;
        moved.target.x += 50.0;
        assert_eq!(at(&cam), at(&moved));
    }

    #[test]
    fn a_background_stops_when_its_plugin_is_turned_off() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.background_shader = "dusk/dusk".into();
        h.app.plugin_shaders.loading = Some(("dusk/dusk".into(), super::Pending::default()));
        assert!(super::allowed(&mut h.app), "an enabled plugin's background may draw");
        assert!(h.app.plugin_shaders.loading.is_some());
        h.app.preferences.disabled_plugins.insert("dusk".into());
        assert!(!super::allowed(&mut h.app), "a turned-off plugin's background must not draw");
        assert!(h.app.plugin_shaders.loading.is_none(), "its pending load is dropped");
        assert_eq!(h.app.plugin_shaders.failed.as_deref(), Some("dusk/dusk"));
    }

    #[test]
    fn a_background_comes_back_when_its_plugin_is_turned_on_again() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.background_shader = "dusk/dusk".into();
        h.app.preferences.disabled_plugins.insert("dusk".into());
        assert!(!super::allowed(&mut h.app));
        assert_eq!(h.app.plugin_shaders.failed.as_deref(), Some("dusk/dusk"));
        h.app.preferences.disabled_plugins.remove("dusk");
        assert!(super::allowed(&mut h.app));
        assert_eq!(h.app.plugin_shaders.failed, None, "the background is tried again");
    }

    #[test]
    fn removing_a_plugin_forgets_its_background() {
        let mut h = crate::scenario::Harness::new();
        h.app.plugin_shaders.loading = Some(("dusk/dusk".into(), super::Pending::default()));
        super::forget(&mut h.app, "other");
        assert!(h.app.plugin_shaders.loading.is_some(), "another plugin's removal leaves it");
        super::forget(&mut h.app, "dusk");
        assert!(h.app.plugin_shaders.loading.is_none());
    }
}
