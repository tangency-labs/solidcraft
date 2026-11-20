//! The model's depth in the headset's own depth buffer, before the controllers, the rays and the
//! belt are drawn: what is behind the model is hidden by it, as in the room. The model is drawn
//! by the window's renderer into a picture of its own (`Renderer::draw_second_view`) with the
//! same projection as the room's, so its depth is the room's once turned round where the
//! renderer's runs reversed.

use eframe::egui_wgpu::wgpu;

/// The pass, for depth pictures of one sample and of several.
pub struct ModelDepth {
    single: (wgpu::BindGroupLayout, wgpu::RenderPipeline),
    multi: (wgpu::BindGroupLayout, wgpu::RenderPipeline),
    /// The picture's size and whether the depth runs reversed.
    uniform: wgpu::Buffer,
}

/// The shader for a depth picture of `kind` (`texture_depth_2d` or the multisampled one).
fn shader(kind: &str) -> String {
    format!(
        r#"
struct D {{ size: vec4<f32> }};
@group(0) @binding(0) var depth: {kind};
@group(0) @binding(1) var<uniform> d: D;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @builtin(frag_depth) f32 {{
    let dims = vec2<f32>(textureDimensions(depth));
    let at = vec2<i32>(clamp(floor(pos.xy * dims / d.size.xy), vec2<f32>(0.0), dims - 1.0));
    let z = textureLoad(depth, at, 0);
    return select(z, 1.0 - z, d.size.z > 0.5);
}}
"#
    )
}

/// The pass's uniform as the shader's `D` takes it: the picture's size, and whether the depth
/// runs reversed.
fn uniform(size: [u32; 2], reversed: bool) -> Vec<u8> {
    [size[0] as f32, size[1] as f32, if reversed { 1.0 } else { 0.0 }, 0.0].iter().flat_map(|v| v.to_le_bytes()).collect()
}

impl ModelDepth {
    /// The pipelines, for pictures in `format` with depth in `depth`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, depth: wgpu::TextureFormat) -> ModelDepth {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_vr_model_depth"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let make = |multisampled: bool| {
            let kind = if multisampled { "texture_depth_multisampled_2d" } else { "texture_depth_2d" };
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("sc_vr_model_depth"),
                source: wgpu::ShaderSource::Wgsl(shader(kind).into()),
            });
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("sc_vr_model_depth"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(16),
                        },
                        count: None,
                    },
                ],
            });
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sc_vr_model_depth"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sc_vr_model_depth"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::empty() })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: depth,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });
            (layout, pipeline)
        };
        ModelDepth { single: make(false), multi: make(true), uniform }
    }

    /// What a pass needs to write the model's depth (`samples` a pixel, `reversed` or not, in
    /// `view`) into a picture of `size` pixels: its pipeline and bind group. The uniform is
    /// written now, so the pass must be submitted before the next eye's is made.
    pub fn bind(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        samples: u32,
        reversed: bool,
        view: &wgpu::TextureView,
        size: [u32; 2],
    ) -> (&wgpu::RenderPipeline, wgpu::BindGroup) {
        let (layout, pipeline) = if samples > 1 { &self.multi } else { &self.single };
        queue.write_buffer(&self.uniform, 0, &uniform(size, reversed));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_vr_model_depth"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: self.uniform.as_entire_binding() },
            ],
        });
        (pipeline, bind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both shaders are valid WGSL, with the two entry points the pipelines name.
    #[test]
    fn the_shaders_are_valid() {
        for kind in ["texture_depth_2d", "texture_depth_multisampled_2d"] {
            let module = wgpu::naga::front::wgsl::parse_str(&shader(kind)).unwrap_or_else(|e| panic!("{kind}: {e}"));
            let names: Vec<&str> = module.entry_points.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, ["vs", "fs"], "{kind}");
            wgpu::naga::valid::Validator::new(wgpu::naga::valid::ValidationFlags::all(), wgpu::naga::valid::Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|e| panic!("{kind}: {e:?}"));
        }
    }

    /// The uniform written is as big as the shader's `D`, for either kind of picture, holds the
    /// size and the reversal, and stays that size for a picture of no size.
    #[test]
    fn the_uniform_is_as_big_as_the_shader_reads_it() {
        for kind in ["texture_depth_2d", "texture_depth_multisampled_2d"] {
            assert_eq!(super::super::tests::wgsl_struct_size(&shader(kind), "D"), 16, "{kind}");
        }
        let bytes = uniform([1920, 1080], true);
        let float = |at: usize| f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        assert_eq!([float(0), float(4), float(8), float(12)], [1920.0, 1080.0, 1.0, 0.0]);
        assert_eq!(uniform([0, 0], false).len(), 16);
        assert_eq!(uniform([u32::MAX; 2], false).len(), 16);
    }

    /// A kind of picture WGSL has not is refused by the parser, not drawn with.
    #[test]
    fn a_depth_picture_of_no_known_kind_is_refused() {
        assert!(wgpu::naga::front::wgsl::parse_str(&shader("texture_2d_of_nothing")).is_err());
    }
}
