//! GPU mip pyramid for backdrop textures: sRGB texture (linear-light filtering),
//! a non-sRGB view for the glass shader (gamma bytes), 13-tap downsample blits.

pub(crate) const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// A backdrop texture with its per-level render views and the sampling view.
pub(crate) struct Pyramid {
    pub texture: wgpu::Texture,
    pub size: [u32; 2],
    /// One view per mip level, for rendering into.
    pub mip_views: Vec<wgpu::TextureView>,
    /// Non-sRGB view over all levels; sampled by the glass shader as gamma bytes.
    pub sample_view: wgpu::TextureView,
    bind_groups: Vec<wgpu::BindGroup>,
}

impl Pyramid {
    pub fn mip_levels(&self) -> u32 {
        self.mip_views.len() as u32
    }
}

pub(crate) struct MipGen {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl MipGen {
    pub fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("mipgen.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("egui_glass_mip"),
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("egui_glass_mip"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("egui_glass_mip"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("egui_glass_mip"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { pipeline, layout, sampler }
    }

    /// Creates a mip-mapped texture of `w` x `h` with the views and bind groups the blits need.
    pub fn create(&self, device: &wgpu::Device, w: u32, h: u32, extra_usage: wgpu::TextureUsages) -> Pyramid {
        let mip_level_count = 32 - w.max(h).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("egui_glass_backdrop"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | extra_usage,
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
        });
        let mip_views: Vec<_> = (0..mip_level_count)
            .map(|level| texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() }))
            .collect();
        let bind_groups = mip_views[..mip_views.len() - 1]
            .iter()
            .map(|view| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("egui_glass_mip"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    ],
                })
            })
            .collect();
        let sample_view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8Unorm), ..Default::default() });
        Pyramid { texture, size: [w, h], mip_views, sample_view, bind_groups }
    }

    /// Fills levels 1.. from level 0.
    pub fn generate(&self, encoder: &mut wgpu::CommandEncoder, pyramid: &Pyramid) {
        for level in 1..pyramid.mip_views.len() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_glass_mip"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &pyramid.mip_views[level],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &pyramid.bind_groups[level - 1], &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
