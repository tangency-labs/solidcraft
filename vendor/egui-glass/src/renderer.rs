use std::sync::atomic::{AtomicU32, Ordering};

use egui::Rect;
use egui_wgpu::{CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use wgpu::util::DeviceExt;

use crate::mipgen::MipGen;
use crate::GlassStyle;

const SLOT_SIZE: u64 = 256; // >= min_uniform_buffer_offset_alignment on all backends
const INITIAL_SLOTS: u32 = 64;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect_min: [f32; 2],
    rect_max: [f32; 2],
    bd_min: [f32; 2],
    bd_max: [f32; 2],
    tint: [f32; 4],
    radius: f32,
    blur: f32,
    refraction: f32,
    edge_width: f32,
    chroma: f32,
    brightness: f32,
    saturation: f32,
    specular: f32,
    border: f32,
    shadow: f32,
    shadow_radius: f32,
    srgb_out: f32,
    light_dir: [f32; 2],
    max_lod: f32,
    backdrop_scale: f32,
    corner_a: [f32; 4], // p, a, b, c
    corner_b: [f32; 4], // d, r, theta3, 0
    fill: [f32; 4],     // colour outside the backdrop rect
    shadow_geo: [f32; 4], // offset, spread, reach, 0
}

/// Smoothed corner geometry in the corner's local frame (px), following the
/// corner-smoothing construction: a cubic Bezier from the edge, a circular arc
/// of radius `r` around the corner centre, and the mirrored Bezier.
/// Returns `[p, a, b, c, d, r, theta3]` where `theta3` is the arc start angle.
pub(crate) fn corner_params(radius: f32, smoothing: f32, budget: f32) -> [f32; 7] {
    let r = radius.clamp(0.0, budget.max(0.0));
    if r <= 0.0 {
        return [0.0; 7];
    }
    let s = smoothing.clamp(0.0, 1.0).min((budget / r - 1.0).max(0.0));
    let p = ((1.0 + s) * r).min(budget);
    let arc_measure = 90.0 * (1.0 - s);
    let arc_len = (arc_measure / 2.0).to_radians().sin() * r * std::f32::consts::SQRT_2;
    let alpha = (90.0 - arc_measure) / 2.0;
    let p3_to_p4 = r * (alpha / 2.0).to_radians().tan();
    let beta = 45.0 * s;
    let c = p3_to_p4 * beta.to_radians().cos();
    let d = c * beta.to_radians().tan();
    let b = (p - arc_len - c - d) / 3.0;
    let a = 2.0 * b;
    let p3 = [p - a - b - c, d];
    let theta3 = (p3[1] - r).atan2(p3[0] - r);
    [p, a, b, c, d, r, theta3]
}

/// GPU state shared by every glass surface. Lives in egui-wgpu's `CallbackResources`.
pub(crate) struct GlassResources {
    pub(crate) mipgen: std::sync::Arc<MipGen>,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    backdrop_view: wgpu::TextureView,
    max_lod: f32,
    /// Off-screen backdrop from [`crate::LiveBackdrop`] and the live frame it belongs to.
    live: Option<(wgpu::TextureView, f32, u64, f32)>,
    /// Which live frame the bind group was built for, if it is bound to a live view.
    bound_live: Option<u64>,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    capacity: u32,
    next_slot: u32,
    last_pass: u64,
    /// Max mip level of whatever backdrop is bound for the current pass.
    current_lod: f32,
    current_backdrop_scale: f32,
    srgb_out: bool,
}

impl GlassResources {
    pub(crate) fn new(render_state: &RenderState, msaa_samples: u32) -> Self {
        let device = &render_state.device;
        let shader = device.create_shader_module(wgpu::include_wgsl!("glass.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("egui_glass"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("egui_glass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("egui_glass"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: render_state.target_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: msaa_samples.max(1), ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("egui_glass"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        // 1x1 mid-grey placeholder until `set_backdrop` is called.
        let placeholder = device.create_texture_with_data(
            &render_state.queue,
            &wgpu::TextureDescriptor {
                label: Some("egui_glass_placeholder"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            Default::default(),
            &[128, 128, 128, 255],
        );
        let backdrop_view = placeholder.create_view(&Default::default());
        let uniforms = Self::create_uniforms(device, INITIAL_SLOTS);
        let bind_group = Self::create_bind_group(device, &layout, &uniforms, &backdrop_view, &sampler);
        Self {
            mipgen: std::sync::Arc::new(MipGen::new(device)),
            pipeline,
            layout,
            sampler,
            backdrop_view,
            max_lod: 0.0,
            live: None,
            bound_live: None,
            uniforms,
            bind_group,
            capacity: INITIAL_SLOTS,
            next_slot: 0,
            last_pass: u64::MAX,
            current_lod: 0.0,
            current_backdrop_scale: 1.0,
            srgb_out: render_state.target_format.is_srgb(),
        }
    }

    fn create_uniforms(device: &wgpu::Device, slots: u32) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("egui_glass_uniforms"),
            size: SLOT_SIZE * slots as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    fn create_bind_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        view: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("egui_glass"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: uniforms,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    pub(crate) fn set_backdrop(&mut self, device: &wgpu::Device, view: wgpu::TextureView, mip_levels: u32) {
        self.backdrop_view = view;
        self.max_lod = mip_levels.saturating_sub(1) as f32;
        self.bind_group = Self::create_bind_group(device, &self.layout, &self.uniforms, &self.backdrop_view, &self.sampler);
        self.bound_live = None;
    }

    /// Use an off-screen render as the backdrop for the glass prepared after this call.
    pub(crate) fn set_live_backdrop(&mut self, view: wgpu::TextureView, mip_levels: u32, live_frame: u64, scale: f32) {
        self.live = Some((view, mip_levels.saturating_sub(1) as f32, live_frame, scale));
    }

    /// Binds the live view when one was produced this pass, else the static backdrop.
    fn bind_current(&mut self, device: &wgpu::Device, new_pass: bool) -> f32 {
        if new_pass {
            // A live frame is consumed by the pass it was produced in.
            if self.live.as_ref().is_some_and(|(_, _, frame, _)| self.bound_live == Some(*frame)) {
                self.live = None;
            }
        }
        match &self.live {
            Some((view, lod, frame, scale)) => {
                self.current_backdrop_scale = *scale;
                if self.bound_live != Some(*frame) {
                    self.bind_group = Self::create_bind_group(device, &self.layout, &self.uniforms, view, &self.sampler);
                    self.bound_live = Some(*frame);
                }
                *lod
            }
            None => {
                self.current_backdrop_scale = 1.0;
                if self.bound_live.is_some() {
                    self.bind_group = Self::create_bind_group(device, &self.layout, &self.uniforms, &self.backdrop_view, &self.sampler);
                    self.bound_live = None;
                }
                self.max_lod
            }
        }
    }

    /// Hands out the next uniform slot for this pass, growing the buffer if needed.
    /// Earlier slots of this pass are copied over so their callbacks still paint.
    fn alloc_slot(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, pass_nr: u64) -> u32 {
        let new_pass = self.last_pass != pass_nr;
        if new_pass {
            self.last_pass = pass_nr;
            self.next_slot = 0;
        }
        if self.next_slot >= self.capacity {
            let old = std::mem::replace(&mut self.uniforms, Self::create_uniforms(device, self.capacity * 2));
            encoder.copy_buffer_to_buffer(&old, 0, &self.uniforms, 0, SLOT_SIZE * self.capacity as u64);
            self.capacity *= 2;
            self.bound_live = Some(u64::MAX); // force a rebuild against the new buffer
        }
        self.current_lod = self.bind_current(device, new_pass);
        let slot = self.next_slot;
        self.next_slot += 1;
        slot
    }
}

/// Registers the glass pipeline with egui-wgpu. Call once, e.g. from `App::new`.
/// `msaa_samples` must match `NativeOptions::multisampling` (0 or 1 when unset).
pub fn init(render_state: &RenderState, msaa_samples: u32) {
    let resources = GlassResources::new(render_state, msaa_samples);
    render_state.renderer.write().callback_resources.insert(resources);
}

/// One glass surface. Created by the widgets; see [`crate::paint_glass`].
pub(crate) struct GlassCallback {
    pub rect: Rect,
    pub backdrop_rect: Rect,
    pub fill: egui::Color32,
    pub style: GlassStyle,
    pub pass_nr: u64,
    slot: AtomicU32,
}

impl GlassCallback {
    pub fn new(rect: Rect, backdrop_rect: Rect, fill: egui::Color32, style: GlassStyle, pass_nr: u64) -> Self {
        Self { rect, backdrop_rect, fill, style, pass_nr, slot: AtomicU32::new(0) }
    }

    fn uniforms(&self, ppp: f32, srgb_out: bool, max_lod: f32) -> Uniforms {
        let s = &self.style;
        let px = |r: Rect| ([r.min.x * ppp, r.min.y * ppp], [r.max.x * ppp, r.max.y * ppp]);
        let (rect_min, rect_max) = px(self.rect);
        let (bd_min, bd_max) = px(self.backdrop_rect);
        let budget = 0.5 * self.rect.width().min(self.rect.height()) * ppp;
        let corner = corner_params(s.corner_radius * ppp, s.corner_smoothing, budget);
        // Color32 is premultiplied; the shader mixes towards straight colours.
        let unpremultiply = |c: egui::Color32| {
            let [r, g, b, a] = c.to_normalized_gamma_f32();
            if a > 0.0 { [r / a, g / a, b / a, a] } else { [0.0; 4] }
        };
        let tint = unpremultiply(s.tint);
        let light = egui::vec2(-0.45, -1.0).normalized();
        Uniforms {
            rect_min,
            rect_max,
            bd_min,
            bd_max,
            tint,
            radius: s.corner_radius * ppp,
            blur: s.blur * ppp,
            refraction: s.refraction * ppp,
            edge_width: s.edge_width * ppp,
            chroma: s.chromatic,
            brightness: s.brightness,
            saturation: s.saturation,
            specular: s.specular,
            border: s.border,
            shadow: s.shadow,
            shadow_radius: s.shadow_radius * ppp,
            srgb_out: srgb_out as u32 as f32,
            light_dir: [light.x, light.y],
            max_lod,
            backdrop_scale: 1.0,
            corner_a: [corner[0], corner[1], corner[2], corner[3]],
            corner_b: [corner[4], corner[5], corner[6], 0.0],
            fill: unpremultiply(self.fill),
            shadow_geo: [s.shadow_offset * ppp, s.shadow_spread * ppp, s.reach.clamp(0.0, 1.0), 0.0],
        }
    }
}

impl CallbackTrait for GlassCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(res) = resources.get_mut::<GlassResources>() else { return Vec::new() };
        let slot = res.alloc_slot(device, encoder, self.pass_nr);
        self.slot.store(slot, Ordering::Relaxed);
        let mut uniforms = self.uniforms(screen.pixels_per_point, res.srgb_out, res.current_lod);
        uniforms.backdrop_scale = res.current_backdrop_scale;
        queue.write_buffer(&res.uniforms, SLOT_SIZE * slot as u64, bytemuck::bytes_of(&uniforms));
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &CallbackResources) {
        let Some(res) = resources.get::<GlassResources>() else { return };
        let offset = (SLOT_SIZE * self.slot.load(Ordering::Relaxed) as u64) as u32;
        pass.set_pipeline(&res.pipeline);
        pass.set_bind_group(0, &res.bind_group, &[offset]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::corner_params;

    #[test]
    fn zero_smoothing_is_a_plain_circular_corner() {
        let [p, a, b, c, d, r, theta3] = corner_params(10.0, 0.0, 100.0);
        assert_eq!((p, r), (10.0, 10.0));
        assert!(a.abs() < 1e-4 && b.abs() < 1e-4 && c.abs() < 1e-4 && d.abs() < 1e-4);
        assert!((theta3 + std::f32::consts::FRAC_PI_2).abs() < 1e-4); // arc starts straight below the centre
    }

    #[test]
    fn full_smoothing_spans_twice_the_radius_and_meets_on_the_diagonal() {
        let [p, a, b, c, d, r, theta3] = corner_params(10.0, 1.0, 100.0);
        assert!((p - 20.0).abs() < 1e-4);
        let p3 = [p - a - b - c, d];
        assert!((p3[0] - p3[1]).abs() < 1e-3, "arc collapses to a point on the diagonal");
        assert!(((p3[0] - r).hypot(p3[1] - r) - r).abs() < 1e-3, "p3 lies on the corner circle");
        assert!((theta3 + 3.0 * std::f32::consts::FRAC_PI_4).abs() < 1e-3);
    }

    #[test]
    fn smoothing_above_one_is_clamped() {
        assert_eq!(corner_params(10.0, 5.0, 100.0), corner_params(10.0, 1.0, 100.0));
    }

    #[test]
    fn radius_and_smoothing_are_capped_by_the_budget() {
        let [p, _, _, _, _, r, _] = corner_params(50.0, 0.6, 20.0);
        assert_eq!(r, 20.0);
        assert!(p <= 20.0 + 1e-4);
    }
}
