//! A startup probe of the installed viewport, including highlights, depth, resolve, and blit.
use super::*;
use crate::{GpuError, Result};

/// The startup check never changes a setting unless a working 1x replacement is proven.
#[derive(Debug, PartialEq, Eq)]
pub enum MsaaCheck {
    Passed,
    Fallback,
    FailedAtOne,
}

impl Renderer {
    /// Check a tiny scene through the installed pipelines and window format. On a blank
    /// multisampled result, replace this renderer only if the same scene works at 1x.
    /// Native only: browsers cannot synchronously poll a device. No result is cached on disk.
    pub fn check_msaa(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<MsaaCheck> {
        self.check_msaa_with(device, queue, 0)
    }

    pub(super) fn check_msaa_with(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, blank_samples: u32) -> Result<MsaaCheck> {
        if self.probe_msaa(device, queue, blank_samples)? {
            return Ok(MsaaCheck::Passed);
        }
        if self.target.samples == 1 {
            return Ok(MsaaCheck::FailedAtOne);
        }
        let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
        let mut replacement = Self::with_window_target(device, GpuTarget { samples: 1, ..self.target }, self.window, None);
        let mut error = None;
        for scope in scopes.into_iter().rev() {
            if let Some(e) = crate::popped(scope.pop()) {
                error.get_or_insert(e);
            }
        }
        if let Some(error) = error {
            return Err(GpuError::Render(error.to_string()));
        }
        if !replacement.probe_msaa(device, queue, blank_samples)? {
            return Ok(MsaaCheck::FailedAtOne);
        }
        replacement.tess = self.tess.take();
        #[cfg(not(target_arch = "wasm32"))]
        {
            replacement.tess_later = self.tess_later.take();
        }
        *self = replacement;
        Ok(MsaaCheck::Fallback)
    }

    fn probe_msaa(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, blank_samples: u32) -> Result<bool> {
        if !matches!(
            self.window.format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            return Err(GpuError::Unsupported("anti-aliasing check colour format".into()));
        }
        let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
        let result = self.probe_pixels(device, queue, blank_samples);
        // Discard the probe's uploads and picture even on errors, before the real scene arrives.
        self.model = Batches::default();
        self.sketch = Batches::default();
        self.highlight = Batches::default();
        self.preview = Batches::default();
        self.ss = None;
        self.ss_key = None;
        self.ss_ready = false;
        self.cached = None;
        let mut error = None;
        for scope in scopes.into_iter().rev() {
            if let Some(e) = crate::popped(scope.pop()) {
                error.get_or_insert(e);
            }
        }
        if let Some(error) = error {
            return Err(GpuError::Render(error.to_string()));
        }
        result
    }

    fn probe_pixels(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, blank_samples: u32) -> Result<bool> {
        const SIZE: u32 = 16;
        let mut model = GpuScene::default();
        let red = model.color([255, 0, 0, 255]);
        for p in [[-0.9, -0.9, 0.0], [0.9, -0.9, 0.0], [0.0, 0.9, 0.0]] {
            model.tri(p, [0.0, 0.0, 1.0], red);
        }
        let mut highlight = GpuScene::default();
        let blue = highlight.color([0, 0, 255, 64]);
        for p in [[-0.8, -0.8, 0.0], [0.8, -0.8, 0.0], [0.8, 0.8, 0.0], [-0.8, -0.8, 0.0], [0.8, 0.8, 0.0], [-0.8, 0.8, 0.0]] {
            highlight.xray_tri(p, [0.0, 0.0, 1.0], blue);
        }
        let slot = |scene| Arc::new(Mutex::new(scene));
        let frame = Frame {
            product_lights: None,
            product: false,
            upload_error: None,
            key: 1,
            slot: slot(Some(model)),
            sketch_key: 2,
            sketch_slot: slot(None),
            hl_key: 3,
            hl_slot: slot(Some(highlight)),
            pv_key: 4,
            pv_slot: slot(None),
            pv_hide: false,
            view_proj: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.5, 1.0]],
            back: [0.0, 0.0, 1.0],
            size_px: [SIZE as f32; 2],
            clip: [0.0; 4],
            cap: [0.0; 4],
            images: Vec::new(),
            clouds: Vec::new(),
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            access: None,
            outline: false,
            eye: [0.0, 0.0, 100.0, 0.0],
            ao_radius: 0.0,
            ao_samples: crate::view::AO_SAMPLES,
            ssaa: 1.0,
            hover_tint: [0.0; 3],
            selection_tint: [0.0; 3],
            layers: Vec::new(),
        };
        let size = wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sc_msaa_check"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.window.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&Default::default());
        self.prepare(device, queue, &mut encoder, &frame);
        // Tests reproduce a driver returning a blank resolve, without changing production draws.
        #[cfg(test)]
        if (blank_samples == 1 || blank_samples == self.target.samples)
            && let Some(resolve) = self.ss.as_ref().map(|ss| ss.resolve.as_ref().unwrap_or(&ss.color))
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: resolve,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
        }
        #[cfg(not(test))]
        let _ = blank_samples;
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            self.paint(&frame, &mut pass, [0.0, 0.0, SIZE as f32, SIZE as f32]);
        }
        // Three interior pixels in one row, padded to the copy alignment.
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_msaa_check_read"),
            size: 256,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { origin: wgpu::Origin3d { x: 7, y: 8, z: 0 }, ..color.as_image_copy() },
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(256), rows_per_image: Some(1) },
            },
            wgpu::Extent3d { width: 3, height: 1, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let slice = read.slice(..);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        // No submission index: a lost device must not turn a stale submission into a panic.
        // Software adapters can take seconds on their first draw. Keep the wait bounded;
        // a timeout remains an inconclusive check and preserves the selected sample count.
        device
            .poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_secs(10)) })
            .map_err(|e| GpuError::Render(format!("anti-aliasing check wait: {e}")))?;
        rx.try_recv()
            .map_err(|e| GpuError::Render(format!("anti-aliasing check callback: {e}")))?
            .map_err(|e| GpuError::Render(format!("anti-aliasing check map: {e}")))?;
        let bytes = slice.get_mapped_range().map_err(|e| GpuError::Render(e.to_string()))?;
        let bgra = matches!(self.window.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        Ok(bytes.get(..12).is_some_and(|pixels| {
            pixels.as_chunks::<4>().0.iter().all(|p| {
                let (red, green, blue) = if bgra { (p[2], p[1], p[0]) } else { (p[0], p[1], p[2]) };
                red > 80 && red > green.saturating_add(40) && blue > 20
            })
        }))
    }
}
