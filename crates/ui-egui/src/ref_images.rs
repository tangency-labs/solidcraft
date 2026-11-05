//! Canvases and decals: the design's reference images. With the GPU viewport they are textured
//! quads in the 3D pass (the model hides them); the CPU viewport draws them over its image.

use solidcraft_engine::geom::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Mesh, Shape, TextureHandle, TextureId, pos2};
use solidcore_record::Vec2;
use solidcraft_engine::doc::canvas::Canvas;

use crate::SolveApp;
use crate::gpu::GpuImage;
use crate::viewport::Proj;

/// Largest texture side; bigger images are scaled down for display.
const MAX_SIDE: u32 = 2048;
/// Grid cells per side, so perspective views bend the image only slightly (CPU path).
const CELLS: usize = 8;

/// Decoded pixels: (data length they came from, version, size, RGBA).
type Decoded = (usize, u64, [u32; 2], Arc<Vec<u8>>);

thread_local! {
    static PIXELS: RefCell<HashMap<u64, Option<Decoded>>> = RefCell::new(HashMap::new());
    /// egui textures for the CPU path, by canvas id, with the version they hold.
    static TEX: RefCell<HashMap<u64, (u64, TextureHandle)>> = RefCell::new(HashMap::new());
    static VERSION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

// Codec features are unified across crates. Enabling bounded HDR preparation must
// not route HDR through this synchronous whole-image canvas decoder.
fn decode_reference(bytes: &[u8]) -> Option<image::DynamicImage> {
    match image::guess_format(bytes).ok()? {
        image::ImageFormat::Png | image::ImageFormat::Jpeg => image::load_from_memory(bytes).ok(),
        _ => None,
    }
}

fn pixels(c: &Canvas) -> Option<Decoded> {
    PIXELS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(d) = m.get(&c.id)
            && d.as_ref().is_none_or(|d| d.0 == c.data.len())
        {
            return d.clone();
        }
        let d = c.bytes().and_then(|b| decode_reference(&b)).map(|i| {
            let i = if i.width() > MAX_SIDE || i.height() > MAX_SIDE { i.thumbnail(MAX_SIDE, MAX_SIDE) } else { i };
            let rgba = i.to_rgba8();
            let v = VERSION.with(|v| {
                v.set(v.get() + 1);
                v.get()
            });
            (c.data.len(), v, [rgba.width(), rgba.height()], Arc::new(rgba.into_raw()))
        });
        m.insert(c.id, d.clone());
        d
    })
}

fn forget_removed(app: &SolveApp) {
    let doc = &app.session.doc;
    PIXELS.with(|m| m.borrow_mut().retain(|id, _| doc.canvases.iter().any(|c| c.id == *id)));
    TEX.with(|t| t.borrow_mut().retain(|id, _| doc.canvases.iter().any(|c| c.id == *id)));
}

/// The visible canvases as GPU quads.
pub fn gpu_images(app: &SolveApp) -> Vec<GpuImage> {
    forget_removed(app);
    let doc = &app.session.doc;
    if doc.canvases.is_empty() || app.session.folder_hidden(crate::browser::CANVASES) {
        return Vec::new();
    }
    let (vals, _) = doc.param_values();
    doc.canvases
        .iter()
        .filter(|c| c.visible && c.opacity > 0.0)
        .filter_map(|c| {
            let plane = doc.canvas_plane(&vals, c).ok()?;
            let (_, version, size, rgba) = pixels(c)?;
            let corners = c.world_corners(&plane).map(|p| [p.x as f32, p.y as f32, p.z as f32]);
            Some(GpuImage { id: c.id, version, size, rgba, corners, opacity: c.opacity.clamp(0.0, 1.0) as f32 })
        })
        .collect()
}

fn texture(ctx: &egui::Context, c: &Canvas) -> Option<TextureId> {
    let (_, version, [w, h], rgba) = pixels(c)?;
    TEX.with(|t| {
        let mut t = t.borrow_mut();
        if let Some((v, h)) = t.get(&c.id)
            && *v == version
        {
            return Some(h.id());
        }
        let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
        let handle = ctx.load_texture(format!("sc_canvas_{}", c.id), img, egui::TextureOptions::LINEAR);
        let id = handle.id();
        t.insert(c.id, (version, handle));
        Some(id)
    })
}

/// CPU viewport only: the images over the rendered model.
pub fn show(app: &SolveApp, ui: &egui::Ui, painter: &egui::Painter, proj: &Proj) {
    if app.viewport.gpu.is_some() {
        return;
    }
    forget_removed(app);
    let doc = &app.session.doc;
    if doc.canvases.is_empty() || app.session.folder_hidden(crate::browser::CANVASES) {
        return;
    }
    let (vals, _) = doc.param_values();
    for c in doc.canvases.iter().filter(|c| c.visible) {
        let Ok(plane) = doc.canvas_plane(&vals, c) else { continue };
        let Some(tex) = texture(ui.ctx(), c) else { continue };
        let [bl, br, _, tl] = c.corners();
        let (u, v) = (br - bl, tl - bl);
        let tint = Color32::from_white_alpha((c.opacity.clamp(0.0, 1.0) * 255.0) as u8);
        let mut mesh = Mesh::with_texture(tex);
        let mut ok = true;
        for j in 0..=CELLS {
            for i in 0..=CELLS {
                let (s, t) = (i as f64 / CELLS as f64, j as f64 / CELLS as f64);
                let p: Vec2 = bl + u * s + v * t;
                let Some(q) = proj.to_screen(plane.to_world(p)) else {
                    ok = false;
                    continue;
                };
                mesh.vertices.push(egui::epaint::Vertex { pos: q, uv: pos2(s as f32, 1.0 - t as f32), color: tint });
            }
        }
        // Behind the camera: skip rather than draw a torn image.
        if !ok {
            continue;
        }
        let w = (CELLS + 1) as u32;
        for j in 0..CELLS as u32 {
            for i in 0..CELLS as u32 {
                let a = j * w + i;
                mesh.add_triangle(a, a + 1, a + w + 1);
                mesh.add_triangle(a, a + w + 1, a + w);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod codec_tests {
    use super::decode_reference;

    #[test]
    fn hdr_feature_does_not_enable_synchronous_canvas_hdr_decoding() {
        let mut hdr = b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y 1 +X 1\n".to_vec();
        hdr.extend_from_slice(&[128, 64, 32, 129]);
        assert!(image::load_from_memory(&hdr).is_ok());
        assert!(decode_reference(&hdr).is_none());
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1).write_to(&mut png, image::ImageFormat::Png).unwrap();
        assert!(decode_reference(png.get_ref()).is_some());
    }
}
