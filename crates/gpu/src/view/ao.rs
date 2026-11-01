//! The optional AO depth prepass. Only opaque model and preview triangles participate: their
//! depth is drawn once into a single-sample depth map the lit fragment samples for occlusion.
//! The map is independent of the window's MSAA, and the depth runs the same way as the
//! viewport's own buffer (see [`super::depth_compare`] and [`super::far_depth`]).
//! Depth testing chooses the nearest surface; an R32Uint colour attachment stores that depth's
//! bits for portable integer texel reads. GLSL cannot load from a depth texture, and float
//! colour attachments are not available on every GL adapter.

use super::{Batches, TriSource, color_layout, depth_compare, far_depth, instance_layout, planned_layout, shared_layout, tri_layout};

pub(super) struct Resources {
    pub layout: wgpu::BindGroupLayout,
    pub bind: wgpu::BindGroup,
    depth: wgpu::TextureView,
    values: wgpu::TextureView,
    size: [u32; 2],
    pipeline: wgpu::RenderPipeline,
    planned_pipeline: wgpu::RenderPipeline,
    instanced_pipeline: wgpu::RenderPipeline,
    instanced_flat_pipeline: wgpu::RenderPipeline,
}

fn texture(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: [u32; 2]) -> (wgpu::TextureView, wgpu::TextureView, wgpu::BindGroup) {
    let texture = |label, format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let depth = texture("sc_ao_depth", wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT).create_view(&Default::default());
    let values = texture("sc_ao_values", wgpu::TextureFormat::R32Uint, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING)
        .create_view(&Default::default());
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sc_ao_depth"),
        layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&values) }],
    });
    (depth, values, bind)
}

impl Resources {
    /// The layout of the depth map's bind group, which the lit pipelines' layout includes.
    pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_ao_depth"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        })
    }

    /// The prepass's pipelines and a 1 x 1 stand-in map, bound with `layout` ([`Resources::layout`]).
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::BindGroupLayout,
        module: &wgpu::ShaderModule,
        reversed: bool,
    ) -> Self {
        let layout = layout.clone();
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_ao_prepass"),
            bind_group_layouts: &[Some(uniform)],
            immediate_size: 0,
        });
        let built = [Some(tri_layout())];
        let evaluated = [Some(planned_layout()), Some(color_layout())];
        let placed = [Some(shared_layout()), Some(instance_layout())];
        let pipeline = |source: TriSource| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sc_ao_prepass"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(source.entry()),
                    buffers: match source {
                        TriSource::Built => &built[..],
                        TriSource::Planned => &evaluated[..],
                        TriSource::Instanced | TriSource::InstancedFlat => &placed[..],
                    },
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(depth_compare(reversed, wgpu::CompareFunction::Less)),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(&source.fragment("fs_ao_depth")),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::R32Uint,
                        blend: None,
                        write_mask: wgpu::ColorWrites::RED,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let planned_pipeline = pipeline(TriSource::Planned);
        let instanced_pipeline = pipeline(TriSource::Instanced);
        let instanced_flat_pipeline = pipeline(TriSource::InstancedFlat);
        let pipeline = pipeline(TriSource::Built);
        let (depth, values, bind) = texture(device, &layout, [1, 1]);
        Self { layout, bind, depth, values, size: [1, 1], pipeline, planned_pipeline, instanced_pipeline, instanced_flat_pipeline }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        scenes: &[(&Batches, &wgpu::BindGroup)],
        radius: f32,
        size_px: [f32; 2],
        reversed: bool,
    ) {
        let enabled = radius.is_finite() && radius > 0.0;
        // Bound memory at large window sizes; normalized coordinates also handle downsampling.
        let size = target_size(enabled, size_px, device.limits().max_texture_dimension_2d);
        if size != self.size {
            (self.depth, self.values, self.bind) = texture(device, &self.layout, size);
            self.size = size;
        }
        if !enabled {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_ao_prepass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.values,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: f64::from(far_depth(reversed).to_bits()), g: 0.0, b: 0.0, a: 0.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(far_depth(reversed)), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        for &(batch, bind) in scenes {
            pass.set_bind_group(0, bind, &[]);
            pass.set_pipeline(&self.pipeline);
            if let Some(tris) = &batch.tris {
                for (buffer, count) in &tris.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                }
            }
            // The depth prepass is left out of the performance overlay's counts, as before.
            let uncounted = super::Tally::default();
            // The blocks' opaque triangles, for the same depth.
            for key in &batch.block_order {
                if let Some(block) = batch.blocks.get(key) {
                    block.draw(&mut pass, super::TriClass::Opaque, &uncounted);
                }
            }
            pass.set_pipeline(&self.planned_pipeline);
            for draw in batch.planned.iter().filter(|d| d.class == super::TriClass::Opaque) {
                pass.set_vertex_buffer(0, draw.mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, draw.colors.slice(..));
                pass.set_index_buffer(draw.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
            }
            for draw in batch.instanced.iter().filter(|d| d.look == super::InstanceLook::Opaque) {
                draw.draw(&mut pass, &self.instanced_pipeline, &self.instanced_flat_pipeline, &uncounted);
            }
        }
    }
}

fn target_size(enabled: bool, pixels: [f32; 2], device_limit: u32) -> [u32; 2] {
    if !enabled {
        return [1, 1];
    }
    let limit = device_limit.clamp(1, 2048);
    let pixels = pixels.map(|s| if s.is_finite() { s.max(1.0) } else { 1.0 });
    let scale = (limit as f32 / pixels[0].max(pixels[1])).min(1.0);
    pixels.map(|s| (s * scale).ceil().clamp(1.0, limit as f32) as u32)
}

#[cfg(test)]
mod tests {
    use super::target_size;
    #[test]
    fn depth_map_is_bounded_and_released_when_disabled() {
        assert_eq!(target_size(true, [6000.0, 3000.0], 8192), [2048, 1024]);
        assert_eq!(target_size(true, [6000.0, 3000.0], 1024), [1024, 512]);
        assert_eq!(target_size(false, [6000.0, 3000.0], 8192), [1, 1]);
        assert_eq!(target_size(true, [f32::NAN, f32::INFINITY], 8192), [1, 1]);
        assert_eq!(target_size(true, [0.0, -1.0], 8192), [1, 1]);
    }
}
