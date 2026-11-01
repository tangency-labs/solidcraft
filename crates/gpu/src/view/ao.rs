//! The optional AO passes. Only opaque model and preview triangles participate: their depth is
//! drawn once into a single-sample depth map, and the occlusion term of the surface that depth
//! holds is drawn into a texture of the same size and blurred by depth (#9327), which the lit
//! fragment reads. The maps are independent of the window's MSAA, and the depth runs the same way
//! as the viewport's own buffer (see [`super::depth_compare`] and [`super::far_depth`]).
//! Depth testing chooses the nearest surface; an R32Uint colour attachment stores that depth's
//! bits for portable integer texel reads. GLSL cannot load from a depth texture, and float
//! colour attachments are not available on every GL adapter.

use super::{Batches, TriSource, color_layout, depth_compare, far_depth, instance_layout, planned_layout, shared_layout, tri_layout};

const TERM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// What the opaque triangles are drawn with in one pass, by where they come from.
struct OpaquePipelines {
    built: wgpu::RenderPipeline,
    planned: wgpu::RenderPipeline,
    instanced: wgpu::RenderPipeline,
    instanced_flat: wgpu::RenderPipeline,
}

/// The maps at their current size. Group 2 of the shader is the depth map and a term: `bind` has
/// the blurred one for the lit pass, `blur_bind` the raw one for the blur.
pub(super) struct Maps {
    pub bind: wgpu::BindGroup,
    blur_bind: wgpu::BindGroup,
    depth: wgpu::TextureView,
    values: wgpu::TextureView,
    raw: wgpu::TextureView,
    smooth: wgpu::TextureView,
}

pub(super) struct Resources {
    pub layout: wgpu::BindGroupLayout,
    pub maps: Maps,
    size: [u32; 2],
    prepass: OpaquePipelines,
    term: OpaquePipelines,
    blur: wgpu::RenderPipeline,
}

fn maps(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: [u32; 2]) -> Maps {
    let view = |label, format, usage| {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
            .create_view(&Default::default())
    };
    let drawn_and_read = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let depth = view("sc_ao_depth", wgpu::TextureFormat::Depth32Float, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let values = view("sc_ao_values", wgpu::TextureFormat::R32Uint, drawn_and_read);
    let raw = view("sc_ao_raw", TERM_FORMAT, drawn_and_read);
    let smooth = view("sc_ao_smooth", TERM_FORMAT, drawn_and_read);
    let group = |label, term| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&values) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(term) },
            ],
        })
    };
    Maps { bind: group("sc_ao_lit", &smooth), blur_bind: group("sc_ao_blur", &raw), depth, values, raw, smooth }
}

/// A term map as a pass's target, cleared to no occlusion.
fn term_target(view: &wgpu::TextureView) -> Option<wgpu::RenderPassColorAttachment<'_>> {
    Some(wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
    })
}

/// The opaque triangles of `batches`, as neither pass counts them in the performance overlay.
fn draw_opaque(pass: &mut wgpu::RenderPass<'_>, pipelines: &OpaquePipelines, binds: [&wgpu::BindGroup; 2], batches: [&Batches; 2]) {
    for (batch, bind) in batches.into_iter().zip(binds) {
        pass.set_bind_group(0, bind, &[]);
        pass.set_pipeline(&pipelines.built);
        if let Some(tris) = &batch.tris {
            for (buffer, count) in &tris.parts {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*count, 0..1);
            }
        }
        let uncounted = super::Tally::default();
        // The blocks' opaque triangles, for the same depth.
        for key in &batch.block_order {
            if let Some(block) = batch.blocks.get(key) {
                block.draw(pass, super::TriClass::Opaque, &uncounted);
            }
        }
        pass.set_pipeline(&pipelines.planned);
        for draw in batch.planned.iter().filter(|d| d.class == super::TriClass::Opaque) {
            pass.set_vertex_buffer(0, draw.mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, draw.colors.slice(..));
            pass.set_index_buffer(draw.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
        }
        for draw in batch.instanced.iter().filter(|d| d.look == super::InstanceLook::Opaque) {
            draw.draw(pass, &pipelines.instanced, &pipelines.instanced_flat, &uncounted);
        }
    }
}

impl Resources {
    /// The layout of the maps' bind groups, which the lit pipelines' layout includes.
    pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
        let entry = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        };
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_ao_maps"),
            entries: &[entry(0, wgpu::TextureSampleType::Uint), entry(1, wgpu::TextureSampleType::Float { filterable: false })],
        })
    }

    /// The passes' pipelines and 1 x 1 stand-in maps, bound with `layout` ([`Resources::layout`]).
    pub fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::BindGroupLayout,
        module: &wgpu::ShaderModule,
        reversed: bool,
    ) -> Self {
        let layout = layout.clone();
        let prepass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_ao_prepass"),
            bind_group_layouts: &[Some(uniform)],
            immediate_size: 0,
        });
        let term_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_ao_term"),
            bind_group_layouts: &[Some(uniform), None, Some(&layout)],
            immediate_size: 0,
        });
        let built = [Some(tri_layout())];
        let evaluated = [Some(planned_layout()), Some(color_layout())];
        let placed = [Some(shared_layout()), Some(instance_layout())];
        let target = |format| [Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::RED })];
        let opaque = |label, layout: &wgpu::PipelineLayout, fragment: &str, format, depth_write_enabled, compare| {
            let pipeline = |source: TriSource| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(layout),
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
                        depth_write_enabled: Some(depth_write_enabled),
                        depth_compare: Some(depth_compare(reversed, compare)),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module,
                        entry_point: Some(&source.fragment(fragment)),
                        targets: &target(format),
                        compilation_options: Default::default(),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
            OpaquePipelines {
                built: pipeline(TriSource::Built),
                planned: pipeline(TriSource::Planned),
                instanced: pipeline(TriSource::Instanced),
                instanced_flat: pipeline(TriSource::InstancedFlat),
            }
        };
        let prepass = opaque("sc_ao_prepass", &prepass_layout, "fs_ao_depth", wgpu::TextureFormat::R32Uint, true, wgpu::CompareFunction::Less);
        // The term only where the prepass's depth is, so each pixel's term is worked out once.
        let term = opaque("sc_ao_term", &term_layout, "fs_ao_term", TERM_FORMAT, false, wgpu::CompareFunction::LessEqual);
        let blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_ao_blur"),
            layout: Some(&term_layout),
            vertex: wgpu::VertexState { module, entry_point: Some("vs_ao_blur"), buffers: &[], compilation_options: Default::default() },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some("fs_ao_blur"),
                targets: &target(TERM_FORMAT),
                compilation_options: Default::default(),
            }),
            multiview_mask: None,
            cache: None,
        });
        let maps = maps(device, &layout, [1, 1]);
        Self { layout, maps, size: [1, 1], prepass, term, blur }
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        binds: [&wgpu::BindGroup; 2],
        radius: f32,
        size_px: [f32; 2],
        reversed: bool,
        batches: [&Batches; 2],
    ) {
        let enabled = radius.is_finite() && radius > 0.0;
        // Bound memory at large window sizes; normalized coordinates also handle downsampling.
        let size = target_size(enabled, size_px, device.limits().max_texture_dimension_2d);
        if size != self.size {
            self.maps = maps(device, &self.layout, size);
            self.size = size;
        }
        if !enabled {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_ao_prepass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.maps.values,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: f64::from(far_depth(reversed).to_bits()), g: 0.0, b: 0.0, a: 0.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.maps.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(far_depth(reversed)), store: wgpu::StoreOp::Store }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        draw_opaque(&mut pass, &self.prepass, binds, batches);
        drop(pass);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_ao_term"),
            color_attachments: &[term_target(&self.maps.raw)],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.maps.depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(2, &self.maps.bind, &[]);
        draw_opaque(&mut pass, &self.term, binds, batches);
        drop(pass);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_ao_blur"),
            color_attachments: &[term_target(&self.maps.smooth)],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.blur);
        pass.set_bind_group(0, binds[0], &[]);
        pass.set_bind_group(2, &self.maps.blur_bind, &[]);
        pass.draw(0..3, 0..1);
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
