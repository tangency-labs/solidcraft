//! Clip both egui paint and GPU pictures to a card without painting over the view behind it.

use std::sync::Arc;

use egui::{CornerRadius, Painter, Rect, Shape, layers::ShapeIdx, pos2};
use egui_wgpu::CallbackTrait;

/// Clip the shapes painted since `start` to the same rounded boundary as the card's chrome.
/// Horizontal bands follow physical pixel rows, keeping adjacent scissor rectangles seamless.
/// GPU pictures are prepared once and only their final composition is clipped.
pub fn clip_card(painter: &Painter, start: ShapeIdx, rect: Rect, radius: CornerRadius) {
    let bands: Arc<[Rect]> = bands(rect, radius, painter.ctx().pixels_per_point()).into();
    painter.ctx().graphics_mut(|graphics| {
        let list = graphics.entry(painter.layer_id());
        let shapes: Vec<_> = list.all_entries().skip(start.0).cloned().collect();
        for (offset, entry) in shapes.iter().enumerate() {
            list.reset_shape(ShapeIdx(start.0 + offset));
            if let Shape::Callback(callback) = &entry.shape
                && delegate(&callback.callback).is_some()
            {
                let rounded = RoundedCallback { original: callback.callback.clone(), bands: bands.clone() };
                list.add(entry.clip_rect, egui_wgpu::Callback::new_paint_callback(callback.rect, rounded).into());
                continue;
            }
            let bounds = entry.shape.visual_bounding_rect();
            for band in bands.iter() {
                let clip = band.intersect(entry.clip_rect);
                if clip.is_positive() && clip.intersects(bounds) {
                    list.add(clip, entry.shape.clone());
                }
            }
        }
    });
}

fn bands(rect: Rect, radius: CornerRadius, scale: f32) -> Vec<Rect> {
    if !rect.is_positive() || !rect.is_finite() || !scale.is_finite() || scale <= 0.0 {
        return Vec::new();
    }
    let max_radius = rect.width().min(rect.height()) * 0.5;
    let [nw, ne, sw, se] = [radius.nw, radius.ne, radius.sw, radius.se].map(|r| f32::from(r).min(max_radius));
    let top = nw.max(ne);
    let bottom = sw.max(se);
    let inset = |radius: f32, distance: f32| {
        let dy = (radius - distance).max(0.0);
        radius - (radius * radius - dy * dy).max(0.0).sqrt()
    };
    // Step integer physical rows independently. Multiplying the previous f32
    // boundary by the scale can round below that row and repeat it forever.
    let top_end = (rect.top() + top).min(rect.bottom());
    let bottom_start = (rect.bottom() - bottom).max(top_end);
    let mut boundaries = vec![rect.top()];
    for (start, end) in [(rect.top(), top_end), (bottom_start, rect.bottom())] {
        if start > boundaries.last().copied().unwrap_or(rect.top()) {
            boundaries.push(start);
        }
        if end <= start {
            continue;
        }
        let scale = f64::from(scale);
        let first = (f64::from(start) * scale).floor();
        let count = (f64::from(end) * scale).ceil() - first;
        // Keep hostile scales bounded. Normal DPI uses one band per pixel row.
        let steps = count.clamp(1.0, 512.0) as usize;
        for row in 1..steps {
            let edge = ((first + row as f64 * count / steps as f64) / scale) as f32;
            if edge > boundaries.last().copied().unwrap_or(start) && edge < end {
                boundaries.push(edge);
            }
        }
        boundaries.push(end);
    }
    let mut result = Vec::new();
    for pair in boundaries.windows(2) {
        let [y, end] = [pair[0], pair[1]];
        let middle = (y + end) * 0.5;
        let left = inset(nw, middle - rect.top()).max(inset(sw, rect.bottom() - middle));
        let right = inset(ne, middle - rect.top()).max(inset(se, rect.bottom() - middle));
        result.push(Rect::from_min_max(pos2(rect.left() + left, y), pos2(rect.right() - right, end)));
    }
    result
}

type Original = Arc<dyn std::any::Any + Send + Sync>;

fn delegate(original: &Original) -> Option<&dyn CallbackTrait> {
    let callback = original.downcast_ref::<egui_wgpu::Callback>()?;
    if let Some(tile) = callback.downcast_ref::<crate::TileCallback>() {
        Some(tile)
    } else {
        callback.downcast_ref::<crate::ProductHighlightCallback>().map(|callback| callback as &dyn CallbackTrait)
    }
}

struct RoundedCallback {
    original: Original,
    bands: Arc<[Rect]>,
}

impl CallbackTrait for RoundedCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        delegate(&self.original).map(|callback| callback.prepare(device, queue, screen, encoder, resources)).unwrap_or_default()
    }

    fn finish_prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        delegate(&self.original).map(|callback| callback.finish_prepare(device, queue, encoder, resources)).unwrap_or_default()
    }

    fn paint(&self, info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, resources: &egui_wgpu::CallbackResources) {
        let Some(callback) = delegate(&self.original) else { return };
        for band in self.bands.iter() {
            let clip_rect = info.clip_rect.intersect(*band);
            if !clip_rect.is_positive() {
                continue;
            }
            let clipped = egui::PaintCallbackInfo { clip_rect, ..info };
            let pixels = clipped.clip_rect_in_pixels();
            if pixels.width_px > 0 && pixels.height_px > 0 {
                pass.set_scissor_rect(pixels.left_px as u32, pixels.top_px as u32, pixels.width_px as u32, pixels.height_px as u32);
                callback.paint(clipped, pass, resources);
            }
        }
        let pixels = info.clip_rect_in_pixels();
        if pixels.width_px > 0 && pixels.height_px > 0 {
            pass.set_scissor_rect(pixels.left_px as u32, pixels.top_px as u32, pixels.width_px as u32, pixels.height_px as u32);
        }
    }
}
