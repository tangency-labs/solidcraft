//! Cached ground coverage from the viewport's existing GPU geometry. The mask is independent
//! of the viewing camera; a bounded separable Gaussian is recomputed only when casters change.

use solidcore_record::Vec3;
use solidcraft_geom::Aabb3;
use solidcraft_render::Camera;
use wgpu::util::DeviceExt;

use super::{
    Batch, Batches, IMG_SIZE, LevelDraw, Tally, TriClass, buffer_init, color_layout, instance_layout, planned_layout, scene_bind, shared_layout,
    tri_layout, upload,
};

const SIZE: u32 = 256;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "projected_shadow_measurement.rs"]
mod measurement;

#[derive(Clone)]
pub struct Projection {
    coverage: Aabb3,
    height: f64,
    up: solidcraft_render::UpAxis,
    axes: [[f32; 4]; 2],
    corners: [[f32; 3]; 4],
    pub fallback: Vec<u8>,
    opacity: f32,
}

impl Projection {
    /// Bounds and height follow the existing ellipse, in the camera's up-axis frame.
    /// Blur sigma is 5 texels (about 2% of the model span), independent of caster height.
    pub fn new(coverage: Aabb3, height: f64, cam: &Camera, opacity: f32, fallback: Vec<u8>) -> Option<Self> {
        if coverage.is_empty() || !coverage.min.is_finite() || !coverage.max.is_finite() || !height.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return None;
        }
        let span = coverage.size().x.max(coverage.size().y);
        if !span.is_finite() || span <= 0.0 {
            return None;
        }
        // More than the 16-texel filter support on every edge: the image border stays clear.
        let extent = span * 1.2;
        let c = coverage.center();
        let lo = [c.x - extent * 0.5, c.y - extent * 0.5];
        let z = height;
        let corners = [
            Vec3::new(lo[0], lo[1], z),
            Vec3::new(lo[0] + extent, lo[1], z),
            Vec3::new(lo[0] + extent, lo[1] + extent, z),
            Vec3::new(lo[0], lo[1] + extent, z),
        ]
        .map(|p| cam.from_frame(p).to_f32());
        let u = cam.from_frame(Vec3::X) / extent;
        let v = cam.from_frame(Vec3::Y) / extent;
        let axes = [[u.x as f32, u.y as f32, u.z as f32, (-lo[0] / extent) as f32], [v.x as f32, v.y as f32, v.z as f32, (-lo[1] / extent) as f32]];
        if !corners.iter().flatten().chain(axes.iter().flatten()).all(|x| x.is_finite()) {
            return None;
        }
        Some(Self { coverage, height, up: cam.up, axes, corners, fallback, opacity })
    }

    /// Expand only projection parameters for a preview; no unchanged model batch is rebuilt.
    pub(super) fn with_preview(&self, preview: Option<Aabb3>) -> Option<Self> {
        let bb = preview.filter(|b| !b.is_empty() && b.min.is_finite() && b.max.is_finite())?;
        let cam = Camera { up: self.up, ..Default::default() };
        let mut coverage = self.coverage;
        for i in 0..8 {
            coverage.add(cam.to_frame(Vec3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            )));
        }
        Self::new(coverage, self.height, &cam, self.opacity, Vec::new())
    }

    pub(super) fn visible(&self, back: [f32; 3]) -> bool {
        let normal = Camera { up: self.up, ..Default::default() }.from_frame(Vec3::Z).to_f32();
        back.iter().zip(normal).map(|(a, b)| a * b).sum::<f32>() > 0.0
    }
}

pub(super) struct Resources {
    #[cfg(test)]
    generation: u64,
    mask: wgpu::TextureView,
    horizontal: wgpu::TextureView,
    output: wgpu::TextureView,
    mask_bind: wgpu::BindGroup,
    horizontal_bind: wgpu::BindGroup,
    pub bind: wgpu::BindGroup,
    mask_uniform: wgpu::Buffer,
    mask_uniform_bind: wgpu::BindGroup,
    moving_uniform: wgpu::Buffer,
    moving_uniform_bind: wgpu::BindGroup,
    directions: [wgpu::BindGroup; 2],
    mask_pipeline: wgpu::RenderPipeline,
    planned_pipeline: wgpu::RenderPipeline,
    instanced_pipeline: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    pub quad: Option<Batch>,
}

impl Resources {
    /// Three RGBA8 images, 768 KiB total. No depth image, compute shader or copied body mesh.
    pub fn new(device: &wgpu::Device, image_layout: &wgpu::BindGroupLayout, palette_layout: &wgpu::BindGroupLayout) -> Option<Self> {
        if device.limits().max_texture_dimension_2d < SIZE {
            return None;
        }
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_shadow_uniform"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let uniform = |bytes: &[u8]| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sc_shadow_uniform"),
                contents: bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sc_shadow_uniform"),
                layout: &uniform_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            (buffer, bind)
        };
        let (mask_uniform, mask_uniform_bind) = uniform(&[0; 112]);
        let (moving_uniform, moving_uniform_bind) = uniform(&[0; 112]);
        let directions = [[1.0f32 / SIZE as f32, 0.0, 0.0, 0.0], [0.0, 1.0 / SIZE as f32, 0.0, 0.0]]
            .map(|direction| uniform(&direction.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>()).1);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_shadow_linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let image = || {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_shadow_mask"),
                size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | if cfg!(test) { wgpu::TextureUsages::COPY_SRC } else { wgpu::TextureUsages::empty() },
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sc_shadow_image"),
                layout: image_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
                ],
            });
            (view, bind)
        };
        let (mask, mask_bind) = image();
        let (horizontal, horizontal_bind) = image();
        let (output, bind) = image();
        let mask_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sc_shadow_mask"),
            source: wgpu::ShaderSource::Wgsl(MASK_SHADER.into()),
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sc_shadow_blur"),
            source: wgpu::ShaderSource::Wgsl(BLUR_SHADER.into()),
        });
        let mask_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_shadow_mask"),
            bind_group_layouts: &[Some(&uniform_layout), Some(palette_layout)],
            immediate_size: 0,
        });
        let blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_shadow_blur"),
            bind_group_layouts: &[Some(image_layout), Some(&uniform_layout)],
            immediate_size: 0,
        });
        let make =
            |layout: &wgpu::PipelineLayout, shader: &wgpu::ShaderModule, vertex, fragment, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("sc_ground_shadow"),
                    layout: Some(layout),
                    vertex: wgpu::VertexState { module: shader, entry_point: Some(vertex), buffers, compilation_options: Default::default() },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some(fragment),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            blend: if fragment == "fs_mask" {
                                let component = wgpu::BlendComponent {
                                    src_factor: wgpu::BlendFactor::One,
                                    dst_factor: wgpu::BlendFactor::One,
                                    operation: wgpu::BlendOperation::Max,
                                };
                                Some(wgpu::BlendState { color: component, alpha: component })
                            } else {
                                None
                            },
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let mask_pipeline = make(&mask_layout, &mask_shader, "vs_mask", "fs_mask", &[Some(tri_layout())]);
        let planned_pipeline = make(&mask_layout, &mask_shader, "vs_mask", "fs_mask", &[Some(planned_layout()), Some(color_layout())]);
        let instanced_pipeline = make(&mask_layout, &mask_shader, "vs_mask_inst", "fs_mask", &[Some(shared_layout()), Some(instance_layout())]);
        let blur = make(&blur_layout, &blur_shader, "vs_blur", "fs_blur", &[]);
        Some(Self {
            #[cfg(test)]
            generation: 0,
            mask,
            horizontal,
            output,
            mask_bind,
            horizontal_bind,
            bind,
            mask_uniform,
            mask_uniform_bind,
            moving_uniform,
            moving_uniform_bind,
            directions,
            mask_pipeline,
            planned_pipeline,
            instanced_pipeline,
            blur,
            quad: None,
        })
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        projection: &Projection,
        clip: [f32; 4],
        batches: [&Batches; 2],
        moving: Option<(&Batches, [[f32; 4]; 4])>,
        fallback_bind: &wgpu::BindGroup,
    ) {
        #[cfg(test)]
        {
            self.generation = self.generation.saturating_add(1);
        }
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        let bytes = |transform: &[[f32; 4]; 4]| {
            projection.axes.iter().flatten().chain(&clip).chain(transform.iter().flatten()).flat_map(|x| x.to_le_bytes()).collect::<Vec<_>>()
        };
        queue.write_buffer(&self.mask_uniform, 0, &bytes(&identity));
        if let Some((_, transform)) = &moving {
            queue.write_buffer(&self.moving_uniform, 0, &bytes(transform));
        }
        let batches: Vec<_> = batches
            .into_iter()
            .map(|batch| (batch, &self.mask_uniform_bind))
            .chain(moving.map(|(batch, _)| (batch, &self.moving_uniform_bind)))
            .collect();
        // Shadow coverage uses every placement, independent of the viewing camera's culling
        // and LOD. Only the small placement records are copied; triangle buffers stay shared.
        let placements: Vec<Vec<LevelDraw>> = batches
            .iter()
            .map(|(batch, _)| {
                batch
                    .instanced
                    .iter()
                    .filter_map(|draw| {
                        let level = draw.levels.first()?;
                        let instances = buffer_init(device, "sc_shadow_instances", &draw.placements, wgpu::BufferUsages::VERTEX)?;
                        Some(LevelDraw {
                            tris: level.tris.clone(),
                            flat: level.flat,
                            error: 0.0,
                            instances,
                            count: u32::try_from(draw.placements.len() / super::INSTANCE_SIZE).unwrap_or(u32::MAX),
                        })
                    })
                    .collect()
            })
            .collect();
        {
            let mut pass = pass(encoder, &self.mask);
            for ((batch, mask_bind), placements) in batches.into_iter().zip(&placements) {
                pass.set_bind_group(0, mask_bind, &[]);
                pass.set_bind_group(1, scene_bind(batch, fallback_bind), &[]);
                pass.set_pipeline(&self.mask_pipeline);
                for triangles in [&batch.tris, &batch.open_tris, &batch.glass, &batch.trans, &batch.ghost].into_iter().flatten() {
                    for (buffer, count) in &triangles.parts {
                        pass.set_vertex_buffer(0, buffer.slice(..));
                        pass.draw(0..*count, 0..1);
                    }
                }
                let uncounted = Tally::default();
                for key in &batch.block_order {
                    if let Some(block) = batch.blocks.get(key) {
                        for class in [TriClass::Opaque, TriClass::Glass, TriClass::Faded] {
                            block.draw_runs(&mut pass, block.class(class), None, &uncounted);
                        }
                    }
                }
                pass.set_pipeline(&self.planned_pipeline);
                for draw in &batch.planned {
                    pass.set_vertex_buffer(0, draw.mesh.vertices.slice(..));
                    pass.set_vertex_buffer(1, draw.colors.slice(..));
                    pass.set_index_buffer(draw.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                }
                pass.set_pipeline(&self.instanced_pipeline);
                for level in placements {
                    level.draw(&mut pass, &uncounted);
                }
            }
        }
        for (target, source, direction) in
            [(&self.horizontal, &self.mask_bind, &self.directions[0]), (&self.output, &self.horizontal_bind, &self.directions[1])]
        {
            let mut pass = pass(encoder, target);
            pass.set_pipeline(&self.blur);
            pass.set_bind_group(0, source, &[]);
            pass.set_bind_group(1, direction, &[]);
            pass.draw(0..3, 0..1);
        }
        let mut quad = Vec::with_capacity(6 * IMG_SIZE);
        let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for i in [0, 1, 2, 0, 2, 3] {
            for value in projection.corners[i].iter().chain(&uv[i]).chain(std::iter::once(&projection.opacity)) {
                quad.extend_from_slice(&value.to_le_bytes());
            }
        }
        self.quad = upload(device, "sc_shadow_quad", &quad, IMG_SIZE);
    }
}

fn pass<'a>(encoder: &'a mut wgpu::CommandEncoder, view: &'a wgpu::TextureView) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("sc_ground_shadow"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

// Gaussian sigma=5, support ±16 texels. Each adjacent pair uses one bilinear fetch at its
// weighted centre, exactly reproducing the dense discrete convolution (17 texture fetches).
const MASK_SHADER: &str = r#"
struct Mask { u: vec4<f32>, v: vec4<f32>, clip: vec4<f32>, model: mat4x4<f32> };
@group(0) @binding(0) var<uniform> mask: Mask;
@group(1) @binding(1) var pal: texture_2d<u32>;
struct M { @builtin(position) pos: vec4<f32>, @location(0) wp: vec3<f32>, @location(1) a:f32 };
fn projected(p: vec3<f32>, c: u32) -> M {
    let q = mask.model * vec4<f32>(p,1.0); var o: M;
    let color = textureLoad(pal, vec2<i32>(i32(c & 2047u), i32(c >> 11u)), 0).x;
    o.pos = vec4<f32>(dot(mask.u,q)*2.0-1.0, dot(mask.v,q)*2.0-1.0, 0.0, 1.0);
    o.wp=q.xyz; o.a=f32((color >> 24u) & 255u) / 255.0; return o;
}
@vertex fn vs_mask(@location(0) p: vec3<f32>, @location(2) c:u32) -> M {
    return projected(p, c);
}
@vertex fn vs_mask_inst(@location(0) p: vec3<f32>, @location(3) a: vec4<f32>, @location(4) b: vec4<f32>, @location(5) c: vec4<f32>, @location(6) color: u32) -> M {
    let q = vec4<f32>(p, 1.0);
    return projected(vec3<f32>(dot(a,q),dot(b,q),dot(c,q)), color);
}
@fragment fn fs_mask(i:M) -> @location(0) vec4<f32> {
    if (length(mask.clip.xyz)>0.5 && dot(mask.clip.xyz,i.wp)>mask.clip.w) { discard; }
    return vec4<f32>(0.0,0.0,0.0,i.a);
}
"#;
const BLUR_SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(1) @binding(0) var<uniform> direction: vec4<f32>;
struct B { @builtin(position) pos:vec4<f32>, @location(0) uv:vec2<f32> };
@vertex fn vs_blur(@builtin(vertex_index) i:u32) -> B {
    let x=f32((i<<1u)&2u); let y=f32(i&2u); var o:B;
    o.pos=vec4<f32>(x*2.0-1.0,1.0-y*2.0,0.0,1.0); o.uv=vec2<f32>(x,y); return o;
}
@fragment fn fs_blur(i:B) -> @location(0) vec4<f32> {
    const offsets=array<f32,8>(1.48500450,3.46505705,5.44522076,7.42555748,9.40612690,11.38698582,13.36818758,15.34978145);
    const weights=array<f32,8>(0.15200668,0.12470146,0.08731414,0.05217913,0.02661346,0.01158481,0.00430376,0.00136448);
    var a=0.07986417*textureSample(src,smp,i.uv).a;
    for(var k=0; k<8; k=k+1) {
        let offset=direction.xy*offsets[k];
        a=a+weights[k]*(textureSample(src,smp,i.uv+offset).a+textureSample(src,smp,i.uv-offset).a);
    }
    return vec4<f32>(0.0,0.0,0.0,a);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn shadow_cache_reuses_camera_motion_refreshes_clipping_and_releases_disabled_images() -> Result<(), Box<dyn std::error::Error>> {
        use super::super::{Frame, GpuScene, GpuTarget, Renderer};
        use std::sync::{Arc, Mutex};
        let (adapter, device, queue) = match crate::open_headless() {
            Ok(device) => device,
            Err(crate::GpuError::NoAdapter(_)) => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        eprintln!("shadow cache adapter: {:?}", adapter.get_info());
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let target = GpuTarget { format: wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false };
        let mut renderer = Renderer::new(&device, target, None);
        let bounds = Aabb3 { min: Vec3::new(-1.0, -1.0, 1.0), max: Vec3::new(1.0, 1.0, 2.0) };
        let mut scene = GpuScene {
            projected_shadow: Some(Arc::new(Projection::new(bounds, 0.99, &Camera::default(), 0.3, Vec::new()).ok_or("projection rejected")?)),
            ..Default::default()
        };
        for point in [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [0.0, 1.0, 1.0]] {
            let color = scene.color([150, 150, 150, 255]);
            scene.tri(point, [0.0, 0.0, 1.0], color);
        }
        let empty = || Arc::new(Mutex::new(None));
        let mut frame = Frame {
            product_lights: None,
            upload_error: None,
            product: false,
            key: 1,
            slot: Arc::new(Mutex::new(Some(scene))),
            sketch_key: 0,
            sketch_slot: empty(),
            hl_key: 0,
            hl_slot: empty(),
            pv_key: 0,
            pv_slot: empty(),
            pv_hide: false,
            preview_bounds: None,
            view_proj: Camera::default().view_proj(1.0, 10.0).to_f32(),
            back: [0.0, 0.0, 1.0],
            size_px: [256.0; 2],
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
        };
        let centre = std::cell::Cell::new(0.0);
        let mut prepare = |frame: &Frame| -> Result<Option<u64>, Box<dyn std::error::Error>> {
            let mut encoder = device.create_command_encoder(&Default::default());
            renderer.prepare(&device, &queue, &mut encoder, frame);
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shadow mask regression"),
                size: u64::from(SIZE * SIZE * 4),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            if let Some(shadow) = &renderer.shadow {
                encoder.copy_texture_to_buffer(
                    shadow.mask.texture().as_image_copy(),
                    wgpu::TexelCopyBufferInfo {
                        buffer: &readback,
                        layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE * 4), rows_per_image: None },
                    },
                    wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                );
            }
            let submitted = queue.submit([encoder.finish()]);
            device.poll(wgpu::PollType::Wait { submission_index: Some(submitted), timeout: Some(std::time::Duration::from_secs(30)) })?;
            if renderer.shadow.is_some() {
                let (send, receive) = std::sync::mpsc::channel();
                readback.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                    let _ = send.send(result);
                });
                device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_secs(30)) })?;
                receive.recv()??;
                let bytes = readback.slice(..).get_mapped_range()?;
                let (sum, count) = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(_, pixel)| pixel[3] > 0)
                    .fold((0_u64, 0_u64), |(sum, count), (i, _)| (sum + i as u64 % u64::from(SIZE), count + 1));
                centre.set(sum as f64 / count.max(1) as f64);
                drop(bytes);
                readback.unmap();
            }
            Ok(renderer.shadow.as_ref().map(|s| s.generation))
        };
        assert_eq!(prepare(&frame)?, Some(1));
        frame.back = [0.0, 0.0, -1.0];
        assert_eq!(prepare(&frame)?, Some(1), "looking below the ground must retain the cached mask");
        frame.back = [0.0, 0.0, 1.0];
        assert_eq!(prepare(&frame)?, Some(1));
        frame.view_proj = Camera { yaw: 1.0, ..Default::default() }.view_proj(1.0, 10.0).to_f32();
        frame.size_px = [512.0; 2];
        assert_eq!(prepare(&frame)?, Some(1));
        let original_centre = centre.get();
        let mut moving = GpuScene::default();
        let color = moving.color([150, 150, 150, 255]);
        for point in [[-1.0, -1.0, 1.0], [1.0, -1.0, 1.0], [0.0, 1.0, 1.0]] {
            moving.tri(point, [0.0, 0.0, 1.0], color);
        }
        frame.moving = Some(super::super::MovingLayer {
            key: 17,
            slot: Arc::new(Mutex::new(Some(moving))),
            transform: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        });
        assert_eq!(prepare(&frame)?, Some(2));
        frame.moving.as_mut().ok_or("moving layer missing")?.transform[3][0] = 0.4;
        assert_eq!(prepare(&frame)?, Some(3), "the same moving geometry at a new position refreshes the mask");
        assert!(centre.get() > original_centre + 5.0, "the moving caster must change actual mask pixels");
        assert_eq!(prepare(&frame)?, Some(3), "a still moving layer reuses its mask");
        frame.moving = None;
        assert_eq!(prepare(&frame)?, Some(4));
        assert!((centre.get() - original_centre).abs() < 1e-6, "ending the drag removes its caster");
        frame.clip = [1.0, 0.0, 0.0, 0.0];
        assert_eq!(prepare(&frame)?, Some(5));
        frame.pv_key += 1;
        frame.preview_bounds = Some(Aabb3 { min: Vec3::new(100.0, 0.0, -20.0), max: Vec3::new(103.0, 4.0, -8.0) });
        assert_eq!(prepare(&frame)?, Some(6));
        frame.key += 1;
        frame.slot = Arc::new(Mutex::new(Some(GpuScene::default())));
        assert_eq!(prepare(&frame)?, None);
        if let Some(error) = pollster::block_on(scope.pop()) {
            return Err(error.to_string().into());
        }
        Ok(())
    }

    #[test]
    fn projection_keeps_the_original_ground_height_and_maps_both_up_axes() {
        for up in [solidcraft_render::UpAxis::Z, solidcraft_render::UpAxis::Y] {
            let cam = Camera { up, ..Default::default() };
            for bottom in [-20.0, 20.0] {
                let bounds = Aabb3 { min: Vec3::new(-1.5, -2.0, bottom), max: Vec3::new(1.5, 2.0, bottom + 12.0) };
                let coverage = bounds.union(&Aabb3 { min: Vec3::new(100.0, 0.0, -40.0), max: Vec3::new(103.0, 4.0, -28.0) });
                let p = Projection::new(coverage, bottom - 0.014, &cam, 0.3, Vec::new()).unwrap();
                for (corner, expected) in p.corners.iter().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]) {
                    let point = Vec3::new(f64::from(corner[0]), f64::from(corner[1]), f64::from(corner[2]));
                    assert!((cam.to_frame(point).z - (bottom - 0.014)).abs() < 1e-5);
                    for (axis, uv) in p.axes.iter().zip(expected) {
                        let value = axis[0] * corner[0] + axis[1] * corner[1] + axis[2] * corner[2] + axis[3];
                        assert!((value - uv).abs() < 1e-5);
                    }
                }
            }
        }
        assert!(Projection::new(Aabb3::EMPTY, 0.0, &Camera::default(), 0.3, Vec::new()).is_none());
    }
}
