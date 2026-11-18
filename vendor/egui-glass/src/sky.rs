//! SolidCraft: a picture behind the app's view, drawn window-sized into a texture of its own. The
//! view shows that texture and the scene backdrop copies it, so glass sees the same picture on
//! every panel. A panorama turns with the camera; a flat picture stays put; rain runs down a
//! frosted window in front of a flat picture. The drops are simulated on the CPU
//! ([`crate::rain`]) and handed to the shader each frame as small textures.

use std::future::Future;

use egui::TextureId;
use egui_wgpu::{wgpu, RenderState};

use crate::mipgen::{MipGen, Pyramid};
use crate::rain::{Rain, DEFAULT_AMOUNT, CELL, CELLS_H, CELLS_W, DROPS_W, MAX_DROPS, WIPED_H, WIPED_W};

/// What part of the picture the window shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SkyView {
    /// The part of a flat picture between two texture coordinates.
    Flat { min: [f32; 2], max: [f32; 2] },
    /// A wrap-around picture (equirectangular, Z up) seen along `forward`. `right` and `up` are
    /// as long as half the window's width and height are at a distance of one.
    Round { right: [f32; 3], up: [f32; 3], forward: [f32; 3] },
    /// A flat picture behind a frosted window with rain on it, `time` seconds in; `amount` is how
    /// hard it rains, 0 to 1 ([`RAIN_AMOUNT`] when nothing says otherwise); `brightness` is how
    /// bright the picture and the fog are, 0 to 1 ([`RAIN_BRIGHTNESS`]).
    Rain { min: [f32; 2], max: [f32; 2], time: f32, amount: f32, brightness: f32 },
}

impl SkyView {
    /// What the shader multiplies the sky's colour by: only rain is dimmed.
    pub fn gain(&self) -> f32 {
        match self {
            SkyView::Rain { brightness, .. } => 0.12 + 0.88 * brightness.clamp(0.0, 1.0).powi(2),
            _ => 1.0,
        }
    }
}

/// How bright the rain's picture is when nothing says otherwise: a dark night, about a quarter of
/// the picture's own light.
pub const RAIN_BRIGHTNESS: f32 = 0.4;

/// How hard it rains when nothing says otherwise: about three times the first tuning's rate.
pub const RAIN_AMOUNT: f32 = DEFAULT_AMOUNT;

const SHADER: &str = r"
struct Params { right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>, flat_uv: vec4<f32>, lod: f32, round: f32, pad: vec2<f32>, gain: f32 }
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Params;
struct VOut { @builtin(position) pos: vec4<f32>, @location(0) ndc: vec2<f32> }
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    var out: VOut;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.ndc = vec2<f32>(x, y);
    return out;
}
fn hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(123.34, 456.21));
    q += dot(q, q + 45.32);
    return fract(q.x * q.y);
}
struct Wet { bend: vec2<f32>, drop: f32 }
@group(0) @binding(3) var drops_tex: texture_2d<f32>;
@group(0) @binding(4) var cells_tex: texture_2d<f32>;
@group(0) @binding(5) var wiped_tex: texture_2d<f32>;
// The drops on the glass near `q` (the window with its height as one, y down), from the grid the
// CPU fills: each cell holds its first drop and how many. A drop is (x, y, radius, how far it
// stretches above as it slides).
fn drops_at(q: vec2<f32>) -> Wet {
    var out: Wet;
    out.bend = vec2<f32>(0.0);
    out.drop = 0.0;
    let cell = vec2<i32>(floor(q / CELL_SIZE));
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let c = cell + vec2<i32>(dx, dy);
            if c.x < 0 || c.y < 0 || c.x >= CELLS_W || c.y >= CELLS_H {
                continue;
            }
            let span = textureLoad(cells_tex, c, 0);
            let start = i32(span.x);
            let n = min(i32(span.y), 24);
            for (var k = 0; k < n; k++) {
                let i = start + k;
                let d = textureLoad(drops_tex, vec2<i32>(i % DROPS_W, i / DROPS_W), 0);
                let r = d.z;
                var v = q - d.xy;
                // A sliding drop is a head with a tail above it, narrower up there.
                if v.y < 0.0 {
                    v.y = v.y / (1.0 + d.w * 1.4);
                    v.x *= 1.0 + clamp(-v.y / r, 0.0, 1.0) * 0.3;
                }
                let len = length(v);
                if len >= r {
                    continue;
                }
                let inside = 1.0 - smoothstep(r * 0.72, r, len);
                if inside > out.drop {
                    out.drop = inside;
                    out.bend = v / r * inside;
                }
            }
        }
    }
    return out;
}
fn frosted(uv: vec2<f32>, lod: f32, spread: f32, grain: f32) -> vec3<f32> {
    // A few taps turned by a per-pixel angle: the fine grain of frosted glass, and no blocks from
    // the wide levels of the picture.
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < 7; i++) {
        let a = f32(i) * 2.399963 + grain * 6.2831853;
        let at = sqrt((f32(i) + 0.5) / 7.0) * spread;
        sum += textureSampleLevel(src, samp, uv + vec2<f32>(cos(a), sin(a) * p.pad.y) * at, lod).rgb;
    }
    return sum / 7.0;
}
fn rain(uv: vec2<f32>, window: vec2<f32>) -> vec3<f32> {
    let q = window * vec2<f32>(p.pad.y, 1.0);
    let wet = drops_at(q);
    let drop = clamp(wet.drop, 0.0, 1.0);
    // Where sliding drops wiped the fog off; it fogs over again slowly.
    let trail = textureSampleLevel(wiped_tex, samp, window, 0.0).r;
    let bend = wet.bend;
    // Each drop is a lens: it shows the picture upside down and small, and nearly sharp.
    let see = uv - bend * vec2<f32>(0.2 / p.pad.y, 0.2);
    let clear = max(trail * 0.72, drop);
    let lod = mix(5.6, 0.6, clear);
    let spread = mix(0.024, 0.0008, clear);
    let grain = hash(floor(window * vec2<f32>(p.right.w, p.up.w)));
    var c3 = frosted(see, lod, spread, grain);
    // The fog on the glass, gone where the water has wiped it.
    let fog = vec3<f32>(0.2, 0.22, 0.28) + c3 * 0.3;
    c3 = mix(c3 * 0.85 + fog * 0.62, c3 * 1.04 + vec3<f32>(0.05, 0.055, 0.07), clear);
    // A drop is a clear lens, a little brighter than the fog around it: it gathers the picture's
    // light into a bright crescent low in it, its top rim is dark, and a small light sits high.
    let len = length(bend);
    c3 += c3 * 0.35 * drop + vec3<f32>(0.035, 0.04, 0.05) * drop;
    let rim = smoothstep(0.55, 1.0, len) * drop;
    c3 *= 1.0 - rim * clamp(-bend.y / max(len, 1e-3), 0.0, 1.0) * 0.45;
    let crescent = smoothstep(0.45, 0.95, len) * clamp(bend.y / max(len, 1e-3), 0.0, 1.0);
    c3 += vec3<f32>(0.5, 0.48, 0.44) * crescent * crescent * drop * 0.5;
    let shine = 1.0 - smoothstep(0.0, 0.25, length(bend + vec2<f32>(0.25, 0.4)));
    c3 += vec3<f32>(0.5, 0.52, 0.58) * shine * drop;
    return c3 * p.gain;
}
@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let window = vec2<f32>(in.ndc.x, -in.ndc.y) * 0.5 + 0.5;
    var uv = p.flat_uv.xy + window * (p.flat_uv.zw - p.flat_uv.xy);
    if p.round > 1.5 {
        return vec4<f32>(rain(uv, window), 1.0);
    }
    if p.round > 0.5 {
        let ray = normalize(p.forward.xyz + p.right.xyz * in.ndc.x + p.up.xyz * in.ndc.y);
        uv = vec2<f32>(atan2(ray.y, ray.x) / 6.2831853 + 0.5, 0.5 - asin(clamp(ray.z, -1.0, 1.0)) / 3.1415927);
    }
    // A level set by hand: the wrap of `u` would otherwise pick a tiny one along the seam.
    return vec4<f32>(textureSampleLevel(src, samp, uv, p.lod).rgb, 1.0);
}
";

struct Target {
    size: [u32; 2],
    /// Gamma bytes as they are, for the pass and for the glass's copy.
    plain: wgpu::TextureView,
    id: TextureId,
    drew: Option<SkyView>,
}

pub(crate) struct SkyResources {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    mipgen: std::sync::Arc<MipGen>,
    image: Option<(Pyramid, wgpu::BindGroup)>,
    target: Option<Target>,
    /// The rain's drops, their grid and the wiped map, as the shader reads them.
    drops: wgpu::Texture,
    cells: wgpu::Texture,
    wiped: wgpu::Texture,
    rain: Option<Rain>,
}

/// A texture the CPU fills each frame for the rain.
fn data_texture(device: &wgpu::Device, label: &str, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Writes `bytes` into the first rows of `texture`, `row` bytes each.
fn write_rows(queue: &wgpu::Queue, texture: &wgpu::Texture, bytes: &[u8], row: u32, rows: u32) {
    if rows == 0 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(rows) },
        wgpu::Extent3d { width: texture.width(), height: rows, depth_or_array_layers: 1 },
    );
}

impl SkyResources {
    fn new(device: &wgpu::Device) -> Self {
        let source = SHADER
            .replace("CELL_SIZE", &format!("{CELL:?}"))
            .replace("CELLS_W", &format!("{CELLS_W}"))
            .replace("CELLS_H", &format!("{CELLS_H}"))
            .replace("DROPS_W", &format!("{DROPS_W}"));
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("egui_glass_sky"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let data = |binding, filterable| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("egui_glass_sky"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
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
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                data(3, false),
                data(4, false),
                data(5, true),
            ],
        });
        let pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("egui_glass_sky"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("egui_glass_sky"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("egui_glass_sky"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui_glass_sky"),
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let drops = data_texture(device, "egui_glass_rain_drops", [DROPS_W, (MAX_DROPS as u32).div_ceil(DROPS_W)], wgpu::TextureFormat::Rgba32Float);
        let cells = data_texture(device, "egui_glass_rain_cells", [CELLS_W, CELLS_H], wgpu::TextureFormat::Rgba32Float);
        let wiped = data_texture(device, "egui_glass_rain_wiped", [WIPED_W, WIPED_H], wgpu::TextureFormat::R8Unorm);
        SkyResources { pipeline, layout, sampler, uniform, mipgen: std::sync::Arc::new(MipGen::new(device)), image: None, target: None, drops, cells, wiped, rain: None }
    }

    /// The window-sized picture as gamma bytes, for the glass's copy.
    pub(crate) fn view(&self) -> Option<wgpu::TextureView> {
        self.target.as_ref().filter(|t| t.drew.is_some()).map(|t| t.plain.clone())
    }
}

/// Runs `make` with the device's errors caught, so a card that refuses what it builds is an error
/// to report and not a crash: the first error the device raised meanwhile.
pub fn guarded<T>(render_state: &RenderState, make: impl FnOnce() -> T) -> Result<T, String> {
    let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| render_state.device.push_error_scope(f));
    let made = make();
    let mut error = None;
    for scope in scopes.into_iter().rev() {
        let mut popped = std::pin::pin!(scope.pop());
        if let std::task::Poll::Ready(Some(e)) = popped.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
            error.get_or_insert(e);
        }
    }
    error.map_or(Ok(made), |e| Err(e.to_string()))
}

/// Uploads the picture (RGBA, `size` pixels). Builds the sky's own pipeline the first time, so it
/// needs no glass; call it inside [`guarded`].
///
/// # Panics
///
/// Panics if `rgba` isn't `size` pixels.
pub fn set_sky_image(render_state: &RenderState, size: [u32; 2], rgba: &[u8]) {
    assert_eq!(rgba.len(), size[0] as usize * size[1] as usize * 4, "the sky's pixels");
    let device = &render_state.device;
    let mut renderer = render_state.renderer.write();
    if renderer.callback_resources.get::<SkyResources>().is_none() {
        renderer.callback_resources.insert(SkyResources::new(device));
    }
    let mipgen = renderer.callback_resources.get::<SkyResources>().expect("inserted above").mipgen.clone();
    let pyramid = mipgen.create(device, size[0], size[1], wgpu::TextureUsages::COPY_DST);
    render_state.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &pyramid.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        rgba,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * size[0]), rows_per_image: Some(size[1]) },
        wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
    );
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui_glass_sky") });
    mipgen.generate(&mut encoder, &pyramid);
    render_state.queue.submit([encoder.finish()]);
    let sky = renderer.callback_resources.get_mut::<SkyResources>().expect("inserted above");
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("egui_glass_sky"),
        layout: &sky.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&pyramid.sample_view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sky.sampler) },
            wgpu::BindGroupEntry { binding: 2, resource: sky.uniform.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&sky.drops.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&sky.cells.create_view(&Default::default())) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&sky.wiped.create_view(&Default::default())) },
        ],
    });
    sky.image = Some((pyramid, bind));
    if let Some(target) = &mut sky.target {
        target.drew = None;
    }
}

/// The mip level that keeps the picture from shimmering: how many picture pixels fall on one of
/// the window's.
fn level(view: &SkyView, image: [u32; 2], window: [u32; 2]) -> f32 {
    let per_pixel = match view {
        SkyView::Flat { min, max } => (max[0] - min[0]).abs() * image[0] as f32 / window[0].max(1) as f32,
        SkyView::Rain { .. } => 0.0,
        SkyView::Round { up, .. } => {
            let half = (up[0] * up[0] + up[1] * up[1] + up[2] * up[2]).sqrt();
            image[0] as f32 / std::f32::consts::TAU * (2.0 * half / window[1].max(1) as f32)
        }
    };
    per_pixel.max(1.0).log2()
}

/// Draws the picture for a window of `size` pixels if that view isn't drawn yet, and returns the
/// texture to show it with. `None` before there is a picture.
pub fn draw_sky(render_state: &RenderState, size: [u32; 2], view: SkyView) -> Option<TextureId> {
    let device = &render_state.device;
    let [w, h] = [size[0].max(1), size[1].max(1)];
    let mut guard = render_state.renderer.write();
    let renderer = &mut *guard;
    let sky = renderer.callback_resources.get_mut::<SkyResources>()?;
    sky.image.as_ref()?;
    if sky.target.as_ref().is_none_or(|t| t.size != [w, h]) {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("egui_glass_sky"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let plain = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8Unorm), ..Default::default() });
        let srgb = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let old = sky.target.take();
        let id = renderer.register_native_texture(device, &srgb, wgpu::FilterMode::Linear);
        if let Some(old) = old {
            renderer.free_texture(&old.id);
        }
        let sky = renderer.callback_resources.get_mut::<SkyResources>()?;
        sky.target = Some(Target { size: [w, h], plain, id, drew: None });
    }
    let sky = renderer.callback_resources.get_mut::<SkyResources>()?;
    let (pyramid, bind) = sky.image.as_ref()?;
    let target = sky.target.as_mut()?;
    if target.drew == Some(view) {
        return Some(target.id);
    }
    let lod = level(&view, pyramid.size, [w, h]);
    if let SkyView::Rain { time, amount, .. } = view {
        let aspect = w as f32 / h as f32;
        let rain = sky.rain.get_or_insert_with(|| Rain::new(aspect, 0x5eed_0f_ba1d, amount));
        rain.set_amount(amount);
        rain.advance(time, aspect);
        let up = rain.upload();
        let queue = &render_state.queue;
        let mut drops: Vec<f32> = up.drops.iter().flatten().copied().collect();
        let rows = (up.drops.len() as u32).div_ceil(DROPS_W);
        drops.resize((rows * DROPS_W * 4) as usize, 0.0);
        write_rows(queue, &sky.drops, bytemuck::cast_slice(&drops), DROPS_W * 16, rows);
        let cells: Vec<f32> = up.cells.iter().flatten().copied().collect();
        write_rows(queue, &sky.cells, bytemuck::cast_slice(&cells), CELLS_W * 16, CELLS_H);
        write_rows(queue, &sky.wiped, &up.wiped, WIPED_W, WIPED_H);
    }
    let (right, up, forward, flat, round, time) = match view {
        SkyView::Flat { min, max } => ([0.0; 3], [0.0; 3], [0.0; 3], [min[0], min[1], max[0], max[1]], 0.0, 0.0),
        SkyView::Round { right, up, forward } => (right, up, forward, [0.0; 4], 1.0, 0.0),
        SkyView::Rain { min, max, time, .. } => ([0.0; 3], [0.0; 3], [0.0; 3], [min[0], min[1], max[0], max[1]], 2.0, time),
    };
    let mut params = [0.0_f32; 24];
    params[..3].copy_from_slice(&right);
    params[4..7].copy_from_slice(&up);
    params[8..11].copy_from_slice(&forward);
    params[12..16].copy_from_slice(&flat);
    params[16] = lod;
    params[17] = round;
    params[18] = time;
    params[19] = w as f32 / h as f32;
    params[20] = view.gain();
    // The window's pixels, for the rain's grain.
    params[3] = w as f32;
    params[7] = h as f32;
    let bytes: Vec<u8> = params.iter().flat_map(|v| v.to_le_bytes()).collect();
    render_state.queue.write_buffer(&sky.uniform, 0, &bytes);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui_glass_sky") });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui_glass_sky"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.plain,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&sky.pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
    render_state.queue.submit([encoder.finish()]);
    target.drew = Some(view);
    Some(target.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_shown_smaller_takes_a_smaller_level() {
        let flat = |x: f32| SkyView::Flat { min: [0.0, 0.0], max: [x, 1.0] };
        assert_eq!(level(&flat(1.0), [1024, 512], [1024, 512]), 0.0);
        assert_eq!(level(&flat(1.0), [1024, 512], [256, 128]), 2.0);
        assert_eq!(level(&flat(0.5), [1024, 512], [2048, 1024]), 0.0, "never below the full picture");
    }

    #[test]
    fn a_panorama_takes_the_level_of_the_sky_one_window_pixel_covers() {
        let round = SkyView::Round { right: [0.5, 0.0, 0.0], up: [0.0, 0.0, 0.5], forward: [0.0, 1.0, 0.0] };
        assert_eq!(level(&round, [2048, 1024], [1600, 1024]), 0.0, "a window taller than the sky's pixels needs the full picture");
        let squeezed = level(&round, [2048, 1024], [1600, 256]);
        assert!((squeezed - (2048.0 / std::f32::consts::TAU / 256.0).log2()).abs() < 1e-5, "{squeezed}");
    }

    #[test]
    fn rain_is_drawn_from_the_whole_picture() {
        let rain = SkyView::Rain { min: [0.0, 0.0], max: [1.0, 1.0], time: 3.0, amount: RAIN_AMOUNT, brightness: RAIN_BRIGHTNESS };
        assert_eq!(level(&rain, [4096, 2048], [400, 200]), 0.0);
        assert_eq!(RAIN_AMOUNT, 0.6);
    }

    /// The background is dark by default, and the brightness setting is what the shader is given.
    #[test]
    fn the_rain_background_is_dark_by_default_and_the_brightness_is_applied() {
        let at = |brightness: f32| SkyView::Rain { min: [0.0, 0.0], max: [1.0, 1.0], time: 0.0, amount: RAIN_AMOUNT, brightness };
        assert!(at(RAIN_BRIGHTNESS).gain() < 0.4, "{}", at(RAIN_BRIGHTNESS).gain());
        assert!(at(0.0).gain() < at(RAIN_BRIGHTNESS).gain() && at(RAIN_BRIGHTNESS).gain() < at(0.7).gain());
        assert!((at(1.0).gain() - 1.0).abs() < 1e-6, "full brightness is the picture as it is");
        assert!(at(0.2) != at(0.4), "a new brightness is drawn again");
        let flat = SkyView::Flat { min: [0.0, 0.0], max: [1.0, 1.0] };
        assert_eq!(flat.gain(), 1.0);
    }
}
