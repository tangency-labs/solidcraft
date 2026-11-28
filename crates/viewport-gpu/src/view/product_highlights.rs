//! Depth-correct selection feedback over independently rendered product pixels.
//! The offscreen target contains only highlights; product colour is never drawn into it.
use super::*;

const DEPTH_SHADER: &str = r#"
struct U { vp: mat4x4<f32> };
@group(0) @binding(0) var<uniform> u: U;
// As viewport.wgsl's `FACE_PUSH` and `face_pushed` (`split_push`).
override FACE_PUSH: f32 = 0.0;
fn face_pushed(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.xy, c.z * (1.0 - FACE_PUSH * 1.1920929e-7), c.w);
}
@vertex
fn vs(@location(0) p: vec3<f32>) -> @builtin(position) vec4<f32> {
    return face_pushed(u.vp * vec4<f32>(p, 1.0));
}
@vertex
fn vs_inst(@location(0) p: vec3<f32>, @location(3) r0: vec4<f32>, @location(4) r1: vec4<f32>, @location(5) r2: vec4<f32>) -> @builtin(position) vec4<f32> {
    let q = vec4<f32>(p, 1.0);
    return face_pushed(u.vp * vec4<f32>(dot(r0,q), dot(r1,q), dot(r2,q), 1.0));
}
@fragment
fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }
"#;

pub(super) struct DepthPipelines {
    built: wgpu::RenderPipeline,
    pbr: wgpu::RenderPipeline,
    planned: wgpu::RenderPipeline,
    instanced: wgpu::RenderPipeline,
    feedback_smooth: wgpu::RenderPipeline,
    feedback_flat: wgpu::RenderPipeline,
}
impl DepthPipelines {
    fn new(device: &wgpu::Device, uniform: &wgpu::BindGroupLayout, target: GpuTarget) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sc_product_highlight_depth"),
            source: wgpu::ShaderSource::Wgsl(DEPTH_SHADER.into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_product_highlight_depth"),
            bind_group_layouts: &[Some(uniform)],
            immediate_size: 0,
        });
        let pipeline = |buffers: &[Option<wgpu::VertexBufferLayout<'_>>], instanced: bool| {
            let (depth_stencil, push) = split_push(target.depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(depth_compare(target.reversed_depth(), wgpu::CompareFunction::Less)),
                stencil: Default::default(),
                bias: depth_bias(target.reversed_depth(), FACE_BIAS),
            }));
            let push = face_push(push);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sc_product_highlight_depth"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(if instanced { "vs_inst" } else { "vs" }),
                    buffers,
                    compilation_options: push_options(&push),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil,
                multisample: wgpu::MultisampleState { count: target.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    targets: &[Some(wgpu::ColorTargetState { format: target.format, blend: None, write_mask: wgpu::ColorWrites::empty() })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // Whole-body feedback keeps its shared mesh and placements on the GPU.
        // Its palette may be translucent, unlike ordinary opaque model instances.
        let feedback_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sc_product_highlight_instanced"),
            source: wgpu::ShaderSource::Wgsl(format!("{SHADER}\n{}", pbr::SOURCE).into()),
        });
        let feedback = |source: TriSource| {
            let fragment = source.fragment("fs_tri");
            let (depth_stencil, push) = split_push(highlight_depth(target.depth));
            let push = face_push(push);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sc_product_highlight_instanced"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &feedback_module,
                    entry_point: Some(source.entry()),
                    buffers: &[Some(shared_layout()), Some(instance_layout())],
                    compilation_options: push_options(&push),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil,
                multisample: wgpu::MultisampleState { count: target.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &feedback_module,
                    entry_point: Some(&fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target.format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            built: pipeline(&[Some(tri_layout())], false),
            pbr: pipeline(&[Some(pbr_layout())], false),
            planned: pipeline(&[Some(planned_layout()), Some(color_layout())], false),
            instanced: pipeline(&[Some(shared_layout()), Some(instance_layout())], true),
            feedback_smooth: feedback(TriSource::Instanced),
            feedback_flat: feedback(TriSource::InstancedFlat),
        }
    }
}

impl Renderer {
    /// Shared product upload and camera setup for ordinary colour and transparent feedback.
    pub(super) fn prepare_product_geometry(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) -> bool {
        // A large product scene can exceed what the device will allocate. Its upload runs under
        // its own error scopes, so the failure reaches the preview instead of the device's
        // uncaptured-error handler, and nothing half-uploaded is drawn.
        let scopes = crate::error_scopes(device);
        let refused = self.product.take(device, queue, &self.uniform, &self.uniform_bgl, frame.key, &frame.slot, None, false).err();
        let caught = crate::caught(scopes);
        let highlight_refused =
            self.pic.highlight.take(device, queue, &self.uniform, &self.uniform_bgl, frame.hl_key, &frame.hl_slot, None, false).err();
        // Only a failed product upload drops the product batches; a refused highlight keeps the
        // product picture and is reported below. A refused scene stays in its slot and keeps its
        // failed key so the same over-budget key is not retried on every repaint frame, while a
        // newer key is uploaded normally. A scope error means the scene was taken and what it
        // uploaded can't be trusted, so its key stays and nothing half-uploaded is drawn.
        if caught.is_some() {
            self.product = Batches { key: Some(frame.key), ..Batches::default() };
        } else if let Some(reason) = &refused {
            self.product = Batches { failed_upload: Some((frame.key, reason.clone())), ..Batches::default() };
        }
        let product_failed = caught.or(refused);
        let failed = product_failed.or(highlight_refused);
        if let Some(error) = &failed {
            log::error!("product preview upload failed: {error}");
            self.refused_upload = Some(error.clone());
            if let Some(Ok(mut sink)) = frame.upload_error.as_ref().map(|sink| sink.lock()) {
                *sink = Some(error.clone());
            }
        }
        queue.write_buffer(
            &self.uniform,
            0,
            &uniform_bytes(frame, frame.size_px[0], frame.size_px[1], self.linear_out, self.target.reversed_depth()),
        );
        failed.is_none()
    }

    /// Prepare a transparent product selection/hover image with its own depth and MSAA.
    /// Emit this instead of the ordinary product callback when live pixels are displayed.
    pub fn prepare_product_highlights(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &Frame) {
        self.activate(0);
        self.pic.isolated = false;
        self.prepare_product_highlights_frame(device, queue, encoder, frame);
    }

    /// Render feedback into the picture belonging to one tiled view. Its uniform writes and
    /// draws must reach the GPU together, like the corresponding shaded tile.
    pub fn prepare_product_highlights_view(&mut self, slot: u64, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        self.activate(slot);
        self.pic.isolated = true;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_product_highlight_view") });
        self.prepare_product_highlights_frame(device, queue, &mut encoder, frame);
        queue.submit(std::iter::once(encoder.finish()));
    }

    fn prepare_product_highlights_frame(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &Frame) {
        self.refused_upload = None;
        let key = frame_key(frame);
        if self.pic.product_highlights && can_reuse(self.pic.ss_ready && self.pic.ss.is_some(), self.pic.ss_key, key) {
            let filter = self.pic.ss_scaled && self.pic.cached.is_none();
            if filter {
                self.cache_picture(device, encoder, frame.size_px);
            }
            self.pic.ss_stats = DrawStats { draw_calls: u32::from(filter), ..DrawStats::default() };
            return;
        }
        self.pic.product_highlights = true;
        self.pic.cached = None;
        self.pic.ss_ready = false;
        self.pic.ss_key = None;
        // Feedback without product depth would light occluded faces as if visible.
        if !frame.product || self.target.depth.is_none() {
            return;
        }
        if self.product_depth.is_none() {
            self.product_depth = Some(DepthPipelines::new(device, &self.uniform_bgl.layout, self.target));
        }
        if !self.prepare_product_geometry(device, queue, frame) {
            return;
        }
        self.pic.ss_ready = self.supersample(device, encoder, frame, false);
        self.pic.ss_key = self.pic.ss_ready.then_some(key);
    }

    /// Alpha-blit only the prepared feedback. There is no shaded-model fallback.
    pub fn paint_product_highlights(&self, frame: &Frame, pass: &mut wgpu::RenderPass<'static>, viewport: [f32; 4]) {
        self.paint_product_highlights_view(0, frame, pass, viewport);
    }

    /// Alpha-blit the feedback prepared for this tile.
    pub fn paint_product_highlights_view(&self, slot: u64, frame: &Frame, pass: &mut wgpu::RenderPass<'static>, viewport: [f32; 4]) {
        if self.picture(slot).is_some_and(|picture| picture.product_highlights && picture.ss_ready) {
            self.paint_view(slot, frame, pass, viewport);
        }
    }

    pub(super) fn draw_product_highlights(&self, pass: &mut wgpu::RenderPass<'static>, tally: &Tally) {
        let Some(depth) = &self.product_depth else { return };
        let triangles = |pass: &mut wgpu::RenderPass<'static>, pipeline: &wgpu::RenderPipeline, batch: &Option<Batch>| {
            if let Some(batch) = batch {
                pass.set_pipeline(pipeline);
                for (buffer, count) in &batch.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                    tally.triangles(*count, 1);
                }
            }
        };
        let lines = |pass: &mut wgpu::RenderPass<'static>, pipeline: &wgpu::RenderPipeline, batch: &Option<Batch>| {
            if let Some(batch) = batch {
                pass.set_pipeline(pipeline);
                for (buffer, count) in &batch.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..*count);
                    tally.segments(*count);
                }
            }
        };
        let model = &self.product;
        pass.set_bind_group(0, scene_bind(model, &self.bind), &[]);
        triangles(pass, &depth.built, &model.tris);
        // Open bodies still occlude feedback; their new upstream batch only
        // changes section crossing counts, which product feedback does not use.
        triangles(pass, &depth.built, &model.open_tris);
        triangles(pass, &depth.pbr, &model.pbr);
        for key in &model.block_order {
            let Some(block) = model.blocks.get(key) else { continue };
            // PBR triangles are built into the same blocks (the product preview's): they occlude
            // feedback too, in the block vertex layout the depth pipeline takes.
            for class in [TriClass::Opaque, TriClass::Pbr] {
                if block.class(class).is_some() {
                    pass.set_pipeline(&depth.built);
                    block.draw(pass, class, tally);
                }
            }
        }
        for draw in model.planned.iter().filter(|draw| draw.class == TriClass::Opaque) {
            pass.set_pipeline(&depth.planned);
            pass.set_vertex_buffer(0, draw.mesh.vertices.slice(..));
            pass.set_vertex_buffer(1, draw.colors.slice(..));
            pass.set_index_buffer(draw.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
            tally.triangles(draw.mesh.index_count, 1);
        }
        // PBR placements are opaque too: they occlude feedback as the built ones do.
        for draw in model.instanced.iter().filter(|draw| matches!(draw.look, InstanceLook::Opaque | InstanceLook::Pbr)) {
            draw.draw(pass, &depth.instanced, &depth.instanced, tally);
        }
        // Render's build_highlight emits self-contained mesh triangles/edges, including
        // whole bodies; it intentionally does not reference the Design model's GPU plans.
        let highlight = &self.pic.highlight;
        pass.set_bind_group(0, scene_bind(highlight, &self.bind), &[]);
        for draw in highlight.hover_instanced.iter().chain(&highlight.selected_instanced).filter(|draw| draw.look == InstanceLook::Opaque) {
            draw.draw(pass, &depth.feedback_smooth, &depth.feedback_flat, tally);
        }
        for key in &highlight.block_order {
            if let Some(block) = highlight.blocks.get(key).filter(|block| block.class(TriClass::Opaque).is_some()) {
                pass.set_pipeline(&self.tri_hl);
                block.draw(pass, TriClass::Opaque, tally);
            }
        }
        triangles(pass, &self.tri_hl, &highlight.tris);
        triangles(pass, &self.tri_hl, &highlight.open_tris);
        lines(pass, &self.line_hidden, &highlight.hidden);
        lines(pass, &self.line, &highlight.lines);
        lines(pass, &self.guide_line, &highlight.guide_lines);
        triangles(pass, &self.trans, &highlight.trans);
        triangles(pass, &self.guide_trans, &highlight.guide_trans);
        triangles(pass, &self.xray, &highlight.xray);
        lines(pass, &self.overlay, &highlight.overlays);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn product_depth_shader_uses_only_the_shared_camera_and_validates() {
        let module = naga::front::wgsl::parse_str(super::DEPTH_SHADER).unwrap();
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
        assert_eq!(module.entry_points.len(), 3);
    }
}
