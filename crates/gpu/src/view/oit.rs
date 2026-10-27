//! Weighted blended order-independent transparency (McGuire and Bavoil, "Weighted Blended
//! Order-Independent Transparency", JCGT 2013) for the model's see-through surfaces: glass (an
//! appearance's opacity) and faded context, instanced or not.
//!
//! Each see-through fragment adds its colour, weighted by its alpha and its depth, into an
//! accumulation target, and multiplies its transparency into a revealage target; a full-screen
//! pass then lays the weighted average over what is behind it, by how much is covered. Nothing is
//! sorted, on the CPU or the GPU, however many see-through triangles there are. The targets share
//! the frame's depth (tested, not written), so the opaque model hides what lies behind it.

use super::{TriSource, color_layout, depth_compare, instance_layout, planned_layout, shared_layout, tri_layout};

/// The accumulated colour (premultiplied, weighted) and alpha.
const ACCUM: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// How much of what is behind still shows (1: all of it).
const REVEAL: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

const COMPOSITE: &str = r#"
@group(0) @binding(0) var accum_t: texture_2d<f32>;
@group(0) @binding(1) var reveal_t: texture_2d<f32>;
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}
@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let reveal = textureLoad(reveal_t, p, 0).r;
    let accum = textureLoad(accum_t, p, 0);
    if (reveal >= 1.0) { discard; }
    return vec4<f32>(accum.rgb / max(accum.a, 1e-5), 1.0 - reveal);
}
"#;

/// The pipelines, made once per target kind.
pub(super) struct Pipelines {
    /// Built triangles ([`tri_layout`]), and instanced meshes smooth and flat.
    pub built: wgpu::RenderPipeline,
    /// Faded and see-through bodies drawn from their plans' evaluated vertices.
    pub planned: wgpu::RenderPipeline,
    pub instanced: wgpu::RenderPipeline,
    pub instanced_flat: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
}

/// The targets, kept while the frame's size stays.
pub(super) struct Targets {
    pub size: [u32; 2],
    accum: wgpu::TextureView,
    accum_resolve: Option<wgpu::TextureView>,
    reveal: wgpu::TextureView,
    reveal_resolve: Option<wgpu::TextureView>,
    bind: wgpu::BindGroup,
}

impl Pipelines {
    /// The pipelines for frames of `format` with `depth` and `samples`, using the viewport's
    /// shader `module` and uniform `layout`; `None` where the device cannot blend into a float
    /// target (the model's see-through surfaces are then sorted as before).
    pub fn new(
        device: &wgpu::Device,
        module: &wgpu::ShaderModule,
        layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
        depth: Option<wgpu::TextureFormat>,
        samples: u32,
    ) -> Option<Pipelines> {
        let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
        let reversed = super::reversed_depth(depth);
        let samples = samples.max(1);
        let multisample = wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false };
        let add =
            wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add };
        let keep = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrc,
            operation: wgpu::BlendOperation::Add,
        };
        let targets = [
            Some(wgpu::ColorTargetState {
                format: ACCUM,
                blend: Some(wgpu::BlendState { color: add, alpha: add }),
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: REVEAL,
                blend: Some(wgpu::BlendState { color: keep, alpha: keep }),
                write_mask: wgpu::ColorWrites::RED,
            }),
        ];
        let built_buffers = [Some(tri_layout())];
        let evaluated = [Some(planned_layout()), Some(color_layout())];
        let placed = [Some(shared_layout()), Some(instance_layout())];
        let pipeline = |label: &str, source: TriSource| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(source.entry()),
                    buffers: match source {
                        TriSource::Built => &built_buffers[..],
                        TriSource::Planned => &evaluated[..],
                        TriSource::Instanced | TriSource::InstancedFlat => &placed[..],
                    },
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil: depth.map(|format| wgpu::DepthStencilState {
                    format,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(depth_compare(reversed, wgpu::CompareFunction::LessEqual)),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample,
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(&source.fragment("fs_oit")),
                    targets: &targets,
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let built = pipeline("sc_oit", TriSource::Built);
        let planned = pipeline("sc_oit_planned", TriSource::Planned);
        let instanced = pipeline("sc_oit_instanced", TriSource::Instanced);
        let instanced_flat = pipeline("sc_oit_instanced_flat", TriSource::InstancedFlat);
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("sc_oit"), entries: &[texture(0), texture(1)] });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_oit_composite"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let composite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sc_oit_composite"),
            source: wgpu::ShaderSource::Wgsl(COMPOSITE.into()),
        });
        // Over the frame (premultiplied over transparent black): the average colour by how much
        // is covered.
        let over = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let composite = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_oit_composite"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState { module: &composite_module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
            depth_stencil: None,
            multisample,
            fragment: Some(wgpu::FragmentState {
                module: &composite_module,
                entry_point: Some("fs"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: Some(over), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let mut failed = None;
        for scope in scopes.into_iter().rev() {
            if let Some(e) = crate::popped(scope.pop()) {
                failed.get_or_insert(e.to_string());
            }
        }
        if let Some(e) = failed {
            log::info!("see-through surfaces are sorted (no order-independent transparency here: {e})");
            return None;
        }
        Some(Pipelines { built, planned, instanced, instanced_flat, composite, bgl })
    }

    /// The targets for a frame of `size` pixels at `samples` per pixel.
    pub fn targets(&self, device: &wgpu::Device, size: [u32; 2], samples: u32) -> Targets {
        let samples = samples.max(1);
        let texture = |label: &str, format, samples, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size[0].max(1), height: size[1].max(1), depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let read = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let pair = |label: &str, format| {
            if samples > 1 {
                (texture(label, format, samples, wgpu::TextureUsages::RENDER_ATTACHMENT), Some(texture(label, format, 1, read)))
            } else {
                (texture(label, format, 1, read), None)
            }
        };
        let (accum, accum_resolve) = pair("sc_oit_accum", ACCUM);
        let (reveal, reveal_resolve) = pair("sc_oit_reveal", REVEAL);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_oit"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(accum_resolve.as_ref().unwrap_or(&accum)) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(reveal_resolve.as_ref().unwrap_or(&reveal)) },
            ],
        });
        Targets { size, accum, accum_resolve, reveal, reveal_resolve, bind }
    }

    /// The pass that gathers the see-through surfaces (`draw` records them), against `depth`.
    pub fn accumulate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        t: &Targets,
        depth: Option<&wgpu::TextureView>,
        stencil: bool,
        draw: impl FnOnce(&mut wgpu::RenderPass<'static>),
    ) {
        let attach = |view, resolve_target, clear| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            })
        };
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sc_oit"),
                color_attachments: &[
                    attach(&t.accum, t.accum_resolve.as_ref(), wgpu::Color::TRANSPARENT),
                    attach(&t.reveal, t.reveal_resolve.as_ref(), wgpu::Color::WHITE),
                ],
                depth_stencil_attachment: depth.map(|view| wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: stencil.then_some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        pass.set_viewport(0.0, 0.0, t.size[0] as f32, t.size[1] as f32, 0.0, 1.0);
        draw(&mut pass);
    }

    /// Lay what was gathered over the frame's `color` (resolved into `resolve`).
    pub fn composite(&self, encoder: &mut wgpu::CommandEncoder, t: &Targets, color: &wgpu::TextureView, resolve: Option<&wgpu::TextureView>) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_oit_composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: resolve,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.composite);
        pass.set_bind_group(0, &t.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
