//! A depth pyramid of the last frame drawn: each texel the farthest depth under it (measured
//! the usual way, 0 at the near plane, whichever way the depth buffer runs), each level half the
//! one before. The GPU cull ([`super::gpu_cull`]) leaves out, while the view moves, a placement
//! whose box lies wholly behind the depth that frame drew where the box would be seen in it.
//!
//! Built from the frame's own depth at the end of each frame drawn into a frame of its own
//! ([`super::Renderer`]'s supersampled frame), with the view it was drawn with.

/// The first level, from a multisampled depth buffer: each texel the farthest of the 2 × 2
/// pixels' samples under it.
const FIRST_MS: &str = r#"
struct P { reversed: u32, src_w: u32, src_h: u32, _p: u32 };
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var depth_in: texture_depth_multisampled_2d;
@group(0) @binding(2) var dst: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let n = textureNumSamples(depth_in);
    var far = 0.0;
    for (var dy = 0u; dy < 2u; dy = dy + 1u) {
        for (var dx = 0u; dx < 2u; dx = dx + 1u) {
            let c = min(id.xy * 2u + vec2<u32>(dx, dy), vec2<u32>(p.src_w - 1u, p.src_h - 1u));
            for (var s = 0u; s < n; s = s + 1u) {
                let d = textureLoad(depth_in, c, i32(s));
                far = max(far, select(d, 1.0 - d, p.reversed == 1u));
            }
        }
    }
    textureStore(dst, id.xy, vec4<f32>(far, 0.0, 0.0, 0.0));
}
"#;

/// The first level, from a single-sampled depth buffer.
const FIRST_SS: &str = r#"
struct P { reversed: u32, src_w: u32, src_h: u32, _p: u32 };
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var depth_in: texture_depth_2d;
@group(0) @binding(2) var dst: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if (id.x >= size.x || id.y >= size.y) { return; }
    var far = 0.0;
    for (var dy = 0u; dy < 2u; dy = dy + 1u) {
        for (var dx = 0u; dx < 2u; dx = dx + 1u) {
            let c = min(id.xy * 2u + vec2<u32>(dx, dy), vec2<u32>(p.src_w - 1u, p.src_h - 1u));
            let d = textureLoad(depth_in, c, 0);
            far = max(far, select(d, 1.0 - d, p.reversed == 1u));
        }
    }
    textureStore(dst, id.xy, vec4<f32>(far, 0.0, 0.0, 0.0));
}
"#;

/// Each further level: each texel the farthest of the 2 × 2 under it.
const DOWN: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<r32float, write>;
@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(dst);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let s = textureDimensions(src) - vec2<u32>(1u, 1u);
    var far = 0.0;
    for (var dy = 0u; dy < 2u; dy = dy + 1u) {
        for (var dx = 0u; dx < 2u; dx = dx + 1u) {
            far = max(far, textureLoad(src, min(id.xy * 2u + vec2<u32>(dx, dy), s), 0).x);
        }
    }
    textureStore(dst, id.xy, vec4<f32>(far, 0.0, 0.0, 0.0));
}
"#;

/// The pyramid's pipelines on one device, for depth of `samples` samples per pixel.
pub(super) struct Kernel {
    samples: u32,
    first_layout: wgpu::BindGroupLayout,
    down_layout: wgpu::BindGroupLayout,
    first: wgpu::ComputePipeline,
    down: wgpu::ComputePipeline,
}

impl Kernel {
    /// The pipelines on `device` for a depth buffer of `samples` samples; `None` (logged) where
    /// the device cannot build them: nothing is left out for being hidden.
    pub(super) fn new(device: &wgpu::Device, samples: u32) -> Option<Kernel> {
        if !super::gpu_cull::supported(device) {
            return None;
        }
        let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
        let multisampled = samples > 1;
        let tex = |binding, sample_type, multisampled| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled },
            count: None,
        };
        let store = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::R32Float,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let first_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_hiz_first"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(1, wgpu::TextureSampleType::Depth, multisampled),
                store(2),
            ],
        });
        let down_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_hiz_down"),
            entries: &[tex(0, wgpu::TextureSampleType::Float { filterable: false }, false), store(1)],
        });
        let pipe = |src: &str, layout: &wgpu::BindGroupLayout| {
            let module =
                device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_hiz"), source: wgpu::ShaderSource::Wgsl(src.into()) });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("sc_hiz"),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("sc_hiz"),
                layout: Some(&pl),
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let kernel = Kernel {
            samples,
            first: pipe(if multisampled { FIRST_MS } else { FIRST_SS }, &first_layout),
            down: pipe(DOWN, &down_layout),
            first_layout,
            down_layout,
        };
        let mut failed = None;
        for scope in scopes.into_iter().rev() {
            if let Some(e) = crate::popped(scope.pop()) {
                failed.get_or_insert(e.to_string());
            }
        }
        if let Some(e) = failed {
            log::warn!("hidden placements are drawn while the view moves: {e}");
            return None;
        }
        Some(kernel)
    }

    /// The samples per pixel of the depth these pipelines read.
    pub(super) fn samples(&self) -> u32 {
        self.samples
    }
}

/// The pyramid of one frame size and the view it was last built for.
pub(super) struct Hiz {
    /// The frame's size (pixels); level 0 is half of it.
    pub(super) frame: [u32; 2],
    pub(super) levels: u32,
    level0: [u32; 2],
    /// Every level, for the cull to read.
    pub(super) view: wgpu::TextureView,
    first: wgpu::BindGroup,
    /// The depth view it reads.
    depth: wgpu::TextureView,
    params: wgpu::Buffer,
    downs: Vec<(wgpu::BindGroup, [u32; 2])>,
    /// The view-projection of the frame it holds the depth of (`None` until built).
    pub(super) vp: Option<[[f32; 4]; 4]>,
    /// Which pyramid this is (the cull binds it again when it changes).
    pub(super) generation: u64,
    /// The frame it was last built in ([`super::Renderer`]'s frame count).
    pub(super) built_in: u64,
}

impl Hiz {
    /// A pyramid for a frame of `frame` pixels whose depth is `depth` (its depth aspect).
    pub(super) fn new(device: &wgpu::Device, kernel: &Kernel, depth: &wgpu::TextureView, frame: [u32; 2], generation: u64) -> Hiz {
        let level0 = [frame[0].div_ceil(2).max(1), frame[1].div_ceil(2).max(1)];
        let levels = 32 - level0[0].max(level0[1]).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sc_hiz"),
            size: wgpu::Extent3d { width: level0[0], height: level0[1], depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let level = |k: u32| texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: k, mip_level_count: Some(1), ..Default::default() });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_hiz_params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let first = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_hiz_first"),
            layout: &kernel.first_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(depth) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&level(0)) },
            ],
        });
        let downs = (1..levels)
            .map(|k| {
                let size = [(level0[0] >> k).max(1), (level0[1] >> k).max(1)];
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sc_hiz_down"),
                    layout: &kernel.down_layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&level(k - 1)) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&level(k)) },
                    ],
                });
                (bind, size)
            })
            .collect();
        let view = texture.create_view(&Default::default());
        Hiz { frame, levels, level0, view, first, depth: depth.clone(), params, downs, vp: None, generation, built_in: 0 }
    }

    /// Whether it reads `depth` (a frame made again at the same size has a new depth texture).
    pub(super) fn reads(&self, depth: &wgpu::TextureView) -> bool {
        self.depth == *depth
    }

    /// Build it in `encoder` from the depth of the frame drawn with `vp` (`reversed`: the depth
    /// buffer is 1 at the near plane), in frame `frame`.
    pub(super) fn build(
        &mut self,
        queue: &wgpu::Queue,
        kernel: &Kernel,
        encoder: &mut wgpu::CommandEncoder,
        vp: [[f32; 4]; 4],
        reversed: bool,
        frame: u64,
    ) {
        let words = [u32::from(reversed), self.frame[0], self.frame[1], 0];
        queue.write_buffer(&self.params, 0, &words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>());
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sc_hiz"), timestamp_writes: None });
        pass.set_pipeline(&kernel.first);
        pass.set_bind_group(0, &self.first, &[]);
        pass.dispatch_workgroups(self.level0[0].div_ceil(8), self.level0[1].div_ceil(8), 1);
        pass.set_pipeline(&kernel.down);
        for (bind, size) in &self.downs {
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        }
        self.vp = Some(vp);
        self.built_in = frame;
    }
}

#[cfg(test)]
mod tests {
    /// The pyramid's shaders parse and validate (a bad shader would only fail at run time, and
    /// nothing hidden would be left out).
    #[test]
    fn the_depth_pyramid_shaders_are_valid() {
        for src in [super::FIRST_MS, super::FIRST_SS, super::DOWN] {
            let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
        }
    }
}
