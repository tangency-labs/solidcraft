//! SolidCraft: a picture behind the app's view, drawn window-sized into a texture of its own. The
//! view shows that texture and the scene backdrop copies it, so glass sees the same picture on
//! every panel. A panorama turns with the camera; a flat picture stays put.

use std::future::Future;

use egui::TextureId;
use egui_wgpu::{wgpu, RenderState};

use crate::mipgen::{MipGen, Pyramid};

/// What part of the picture the window shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SkyView {
    /// The part of a flat picture between two texture coordinates.
    Flat { min: [f32; 2], max: [f32; 2] },
    /// A wrap-around picture (equirectangular, Z up) seen along `forward`. `right` and `up` are
    /// as long as half the window's width and height are at a distance of one.
    Round { right: [f32; 3], up: [f32; 3], forward: [f32; 3] },
}

const SHADER: &str = r"
struct Params { right: vec4<f32>, up: vec4<f32>, forward: vec4<f32>, flat_uv: vec4<f32>, lod: f32, round: f32, pad: vec2<f32> }
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
@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let window = vec2<f32>(in.ndc.x, -in.ndc.y) * 0.5 + 0.5;
    var uv = p.flat_uv.xy + window * (p.flat_uv.zw - p.flat_uv.xy);
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
}

impl SkyResources {
    fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("egui_glass_sky"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
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
            size: 80,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        SkyResources { pipeline, layout, sampler, uniform, mipgen: std::sync::Arc::new(MipGen::new(device)), image: None, target: None }
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
    let (right, up, forward, flat, round) = match view {
        SkyView::Flat { min, max } => ([0.0; 3], [0.0; 3], [0.0; 3], [min[0], min[1], max[0], max[1]], 0.0),
        SkyView::Round { right, up, forward } => (right, up, forward, [0.0; 4], 1.0),
    };
    let mut params = [0.0_f32; 20];
    params[..3].copy_from_slice(&right);
    params[4..7].copy_from_slice(&up);
    params[8..11].copy_from_slice(&forward);
    params[12..16].copy_from_slice(&flat);
    params[16] = lod;
    params[17] = round;
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
        let rain = SkyView::Rain { min: [0.0, 0.0], max: [1.0, 1.0], time: 3.0, amount: RAIN_AMOUNT };
        assert_eq!(level(&rain, [4096, 2048], [400, 200]), 0.0);
        assert_eq!(RAIN_AMOUNT, 0.6);
    }
}
