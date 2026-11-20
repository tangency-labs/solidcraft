//! One more view of the scenes a frame uploaded, drawn from another eye point into textures of
//! its own: what a headset's eye sees of the model the window shows. The window's own picture,
//! its cache and its statistics are left as they were.

use super::*;

/// The textures a second view is drawn into. The caller keeps one between frames
/// ([`Renderer::draw_second_view`] makes it, and makes it again when the size or the renderer's
/// sample count changes).
pub struct ViewTarget {
    frame: Supersampled,
    /// The renderer's sample count the textures were made with: a renderer whose multisampling
    /// failed is replaced by a one-sample one (`check_msaa`), whose pipelines cannot draw into
    /// them.
    samples: u32,
    /// Its depth runs reversed (the renderer's depth format).
    reversed: bool,
    /// The see-through surfaces' targets, made when a scene first has such surfaces.
    oit: Option<oit::Targets>,
}

impl ViewTarget {
    /// The picture's size in pixels.
    pub fn size(&self) -> [u32; 2] {
        self.frame.size
    }

    /// The picture: one sample a pixel in the scene target's format, transparent where nothing
    /// was drawn, its colours multiplied by their alpha.
    pub fn picture(&self) -> &wgpu::TextureView {
        self.frame.resolve.as_ref().unwrap_or(&self.frame.color)
    }

    /// The picture's depth for a later pass to read: its sample count, whether it runs reversed
    /// (1 near, 0 far: [`reversed_depth`]), and its depth aspect. Kept from the last draw.
    pub fn depth(&self) -> Option<(u32, bool, &wgpu::TextureView)> {
        self.frame.depth_read.as_ref().map(|view| (self.samples, self.reversed, view))
    }

    /// The picture bound for the renderer's own copy pipeline (tests read it back through that).
    #[cfg(test)]
    pub(super) fn bind(&self) -> &wgpu::BindGroup {
        &self.frame.bind
    }

    /// The sample count the textures were made with.
    #[cfg(test)]
    pub(super) fn samples(&self) -> u32 {
        self.samples
    }
}

impl Renderer {
    /// Draw the scenes the last [`Renderer::prepare`] uploaded as `frame`'s camera sees them,
    /// into `target` at `size` pixels, within `scissor` (x, y, width, height; an empty one draws
    /// nothing but the clear), with the ground grid and the ground shadow only when `ground` (a
    /// headset's room has a floor of its own), and submit that at once. `false` when nothing was drawn: a product
    /// frame, or a device with no memory for the picture.
    ///
    /// `frame` is the window's frame with another camera (`view_proj`, `turned_depth`, `eye`) and
    /// no ambient occlusion, which is prepared for the window's own view only. The uniform, the
    /// culled placements and a sorted glass order are written for this view and submitted with
    /// it, so the window's next `prepare` finds nothing of it left but work already done: it
    /// writes its own again before it draws.
    pub fn draw_second_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        size: [u32; 2],
        scissor: [u32; 4],
        ground: bool,
        target: &mut Option<ViewTarget>,
    ) -> bool {
        if frame.product {
            return false;
        }
        let max = self.max_texture.max(1);
        let size = [size[0].clamp(1, max), size[1].clamp(1, max)];
        let samples = self.target.samples;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("sc_second_view") });
        if target.as_ref().is_none_or(|t| t.frame.size != size || t.samples != samples || t.reversed != self.target.reversed_depth()) {
            // The old picture goes before the new one is asked for.
            *target = None;
            match allocated(device, || self.supersampled(device, size, true)) {
                Ok(made) => {
                    clear_resolve_targets(&mut encoder, made.resolve.as_ref());
                    *target = Some(ViewTarget { frame: made, samples, reversed: self.target.reversed_depth(), oit: None });
                }
                Err(e) => {
                    log::warn!("the second view is not drawn: the GPU refused a {} × {} picture: {e}", size[0], size[1]);
                    return false;
                }
            }
        }
        let Some(view) = target.as_mut() else { return false };
        // See-through surfaces unsorted where the device can and has the memory, as in the window.
        let mut see_through = self.oit.is_some() && (self.model.see_through() || (self.moving_on && self.moving.see_through()));
        if see_through
            && view.oit.as_ref().is_none_or(|t| t.size != size)
            && let Some(p) = &self.oit
        {
            view.oit = None;
            match allocated(device, || p.targets(device, size, samples)) {
                Ok(targets) => {
                    clear_resolve_targets(&mut encoder, targets.resolves());
                    view.oit = Some(targets);
                }
                Err(e) => {
                    log::warn!("the second view's see-through surfaces are sorted: the GPU refused their targets: {e}");
                    see_through = false;
                }
            }
        }
        let [w, h] = [size[0] as f32, size[1] as f32];
        let reversed = self.target.reversed_depth();
        if !see_through {
            self.model.sort_glass(queue, frame.back);
        }
        if self.moving_on
            && let Some(mv) = &frame.moving
        {
            if !see_through {
                self.moving.sort_glass(queue, local_direction(&mv.transform, frame.back));
            }
            queue.write_buffer(&self.moving_uniform, 0, &uniform_bytes_placing(frame, w, h, self.linear_out, reversed, mv.transform));
        }
        self.model.cull(device, queue, &frame.view_proj, [w, h]);
        queue.write_buffer(&self.uniform, 0, &uniform_bytes(frame, w, h, self.linear_out, reversed));

        let [sx, sy] = [scissor[0].min(size[0]), scissor[1].min(size[1])];
        let [sw, sh] = [scissor[2].min(size[0] - sx), scissor[3].min(size[1] - sy)];
        let visible = sw > 0 && sh > 0;
        let stencil = self.target.depth.is_some_and(|f| f.has_stencil_aspect());
        // Kept: the headset hides its controllers and signs behind the model by it.
        let keep = wgpu::StoreOp::Store;
        let far = far_depth(reversed);
        let ss = &view.frame;
        let pass = |encoder: &mut wgpu::CommandEncoder, first: bool| {
            let load = |clear| if first { wgpu::LoadOp::Clear(clear) } else { wgpu::LoadOp::Load };
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_second_view"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &ss.color,
                        depth_slice: None,
                        resolve_target: ss.resolve.as_ref(),
                        ops: wgpu::Operations { load: load(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: ss.depth.as_ref().map(|view| wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations { load: load_depth(first, far), store: keep }),
                        stencil_ops: stencil.then_some(wgpu::Operations { load: load_depth(first, 0), store: keep }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            pass.set_viewport(0.0, 0.0, w, h, 0.0, 1.0);
            if visible {
                pass.set_scissor_rect(sx, sy, sw, sh);
            }
            pass
        };
        // Not counted: the statistics are the window's.
        let tally = Tally::default();
        // Reset below, before anything returns (and by the window's next `prepare` whatever
        // happens): the window's own pictures always have the ground. A drag that ended since the
        // window's last `prepare` has no moving layer to draw: its uniform is the window's.
        self.ground_hidden = !ground;
        let moving_on = self.moving_on;
        self.moving_on &= frame.moving.is_some();
        match (&self.oit, &view.oit, see_through && visible) {
            (Some(p), Some(t), true) => {
                {
                    let mut first = pass(&mut encoder, true);
                    self.draw(frame, &mut first, Phase::BeforeSeeThrough, &tally);
                }
                p.accumulate(&mut encoder, t, ss.depth.as_ref(), stencil, |pass| {
                    pass.set_scissor_rect(sx, sy, sw, sh);
                    self.draw_see_through(pass, p, &tally);
                });
                p.composite(&mut encoder, t, &ss.color, ss.resolve.as_ref());
                let mut over = pass(&mut encoder, false);
                self.draw(frame, &mut over, Phase::AfterSeeThrough, &tally);
            }
            _ => {
                let mut first = pass(&mut encoder, true);
                if visible {
                    self.draw(frame, &mut first, Phase::All, &tally);
                }
            }
        }
        self.ground_hidden = false;
        self.moving_on = moving_on;
        queue.submit([encoder.finish()]);
        true
    }
}
