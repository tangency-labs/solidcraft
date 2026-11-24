//! Canvases and decals: the design's reference images. With the GPU viewport they are textured
//! quads in the 3D pass (the model hides them); the CPU viewport draws them over its image.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use egui::{Color32, Mesh, Shape, TextureHandle, TextureId, pos2};
use solidcraft_engine::doc::{blob::Blob, canvas::Canvas};

use crate::SolveApp;
use crate::gpu::GpuImage;
use crate::viewport::Proj;

/// Largest texture side; bigger images are scaled down for display.
const MAX_SIDE: u32 = 2048;
/// Grid cells per side, so perspective views bend the image only slightly (CPU path).
const CELLS: usize = 8;

/// Decoded pixels: (version, size, RGBA).
type Decoded = (u64, [u32; 2], Arc<Vec<u8>>);

struct Cached {
    /// Retain the immutable source so its allocation cannot be reused for a different picture.
    data: Blob,
    /// The orientation the pixels were turned by (#19112): a canvas can share its data with one turned
    /// otherwise (another open document's, or one saved before orientations were).
    orientation: u8,
    decoded: Option<Decoded>,
}

thread_local! {
    static PIXELS: RefCell<HashMap<u64, Cached>> = RefCell::new(HashMap::new());
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

/// `c`'s picture decoded and turned by its orientation (#19112), at most `MAX_SIDE` on a side.
fn pixels(c: &Canvas) -> Option<Decoded> {
    PIXELS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some(cached) = m.get(&c.id)
            && std::ptr::eq(cached.data.as_bytes(), c.bytes())
            && cached.orientation == c.orientation
        {
            return cached.decoded.clone();
        }
        let d = decode_reference(c.bytes()).map(|mut i| {
            solidcraft_engine::render::image_orientation::turn(&mut i, c.orientation);
            let i = if i.width() > MAX_SIDE || i.height() > MAX_SIDE { i.thumbnail(MAX_SIDE, MAX_SIDE) } else { i };
            let rgba = i.to_rgba8();
            let v = VERSION.with(|v| {
                v.set(v.get() + 1);
                v.get()
            });
            (v, [rgba.width(), rgba.height()], Arc::new(rgba.into_raw()))
        });
        m.insert(c.id, Cached { data: c.data.clone(), orientation: c.orientation, decoded: d.clone() });
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
    let state = app.session.model.state();
    doc.canvases
        .iter()
        .filter(|c| c.visible && c.opacity > 0.0)
        .filter_map(|c| {
            // An anchored canvas follows its sketch's points.
            let corners = doc.canvas_corners(&vals, c, &state).ok()?;
            let (version, size, rgba) = pixels(c)?;
            let corners = corners.map(|p| [p.x as f32, p.y as f32, p.z as f32]);
            Some(GpuImage { id: c.id, version, size, rgba, corners, opacity: c.opacity.clamp(0.0, 1.0) as f32 })
        })
        .collect()
}

fn texture(ctx: &egui::Context, c: &Canvas) -> Option<TextureId> {
    let (version, [w, h], rgba) = pixels(c)?;
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
    let state = app.session.model.state();
    for c in doc.canvases.iter().filter(|c| c.visible) {
        let Ok([bl, br, _, tl]) = doc.canvas_corners(&vals, c, &state) else { continue };
        let Some(tex) = texture(ui.ctx(), c) else { continue };
        let arr = |p: solidcore_record::Vec3| [p.x, p.y, p.z];
        let (corner, u, v) = (arr(bl), arr(br - bl), arr(tl - bl));
        let tint = Color32::from_white_alpha((c.opacity.clamp(0.0, 1.0) * 255.0) as u8);
        let mut mesh = Mesh::with_texture(tex);
        let mut ok = true;
        for j in 0..=CELLS {
            for i in 0..=CELLS {
                let (s, t) = (i as f64 / CELLS as f64, j as f64 / CELLS as f64);
                // The point on the image's quad, from the kernel.
                let [x, y, z] = solidcore_ops::affine::parallelogram_point(corner, u, v, s, t);
                let Some(q) = proj.to_screen(solidcore_record::Vec3::new(x, y, z)) else {
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
    fn design_tabs_keep_their_own_canvas_pixels() {
        use serde_json::json;

        let picture = |color| {
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(2, 2, image::Rgba(color)).write_to(&mut png, image::ImageFormat::Png).unwrap();
            png.into_inner()
        };
        let red = picture([255, 0, 0, 255]);
        let blue = picture([0, 0, 255, 255]);
        assert_eq!(red.len(), blue.len(), "different pictures of equal byte length reproduce the cache collision");
        let mut h = crate::scenario::Harness::new();
        h.app.run("canvas.insert", json!({"path": crate::scenario::image_file(&red), "plane": "XY"})).unwrap();
        let first = super::gpu_images(&h.app).remove(0);
        assert_eq!(&first.rgba[..4], &[255, 0, 0, 255]);
        let first_texture = super::texture(&h.ctx, &h.app.session.doc.canvases[0]).unwrap();

        crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
        h.app.run("canvas.insert", json!({"path": crate::scenario::image_file(&blue), "plane": "XY"})).unwrap();
        let second = super::gpu_images(&h.app).remove(0);
        assert_eq!(first.id, second.id, "each design starts with the same canvas id");
        assert_eq!(&second.rgba[..4], &[0, 0, 255, 255], "the second tab must show its blue picture");
        let second_texture = super::texture(&h.ctx, &h.app.session.doc.canvases[0]).unwrap();
        assert_ne!(first.version, second.version);
        assert_ne!(first_texture, second_texture);

        h.app.run("view.previous_tab", json!({})).unwrap();
        let restored = super::gpu_images(&h.app).remove(0);
        assert_eq!(&restored.rgba[..4], &[255, 0, 0, 255], "returning to the first tab must restore its red picture");
        assert_ne!(second.version, restored.version);
        assert_ne!(second_texture, super::texture(&h.ctx, &h.app.session.doc.canvases[0]).unwrap());
    }

    #[test]
    fn replaced_canvas_data_retries_a_failed_decode_and_clones_reuse_pixels() {
        use serde_json::json;

        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app.run("canvas.insert", json!({"path": crate::scenario::image_file(png.get_ref())})).unwrap();
        let valid = h.app.session.doc.canvases[0].clone();
        let mut invalid = valid.clone();
        invalid.data = vec![0; valid.data.len()].into();
        assert!(super::pixels(&invalid).is_none());
        assert!(super::pixels(&invalid.clone()).is_none());

        let (version, size, rgba) = super::pixels(&valid).expect("different data must be decoded after a failure");
        assert_eq!(size, [2, 2]);
        assert_eq!(&rgba[..4], &[0, 0, 255, 255]);
        let (reused_version, _, reused_rgba) = super::pixels(&valid.clone()).unwrap();
        assert_eq!(version, reused_version, "document clones share the immutable image data");
        assert!(std::sync::Arc::ptr_eq(&rgba, &reused_rgba));
    }

    #[test]
    fn gpu_images_include_canvases_on_model_planes() {
        use serde_json::json;

        let mut h = crate::scenario::Harness::new();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(2, 2).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let data = crate::scenario::image_file(png.get_ref());
        h.app
            .run(
                "construct.plane.three_points",
                json!({"a": {"point": [0, 0, 5]}, "b": {"point": [1, 0, 5]}, "c": {"point": [0, 1, 5]}, "name": "Three"}),
            )
            .unwrap();
        for plane in ["XY", "Three"] {
            h.app.run("canvas.insert", json!({"path": data, "plane": plane, "name": plane, "width": 40})).unwrap();
        }
        let images = super::gpu_images(&h.app);
        assert_eq!(images.len(), 2, "both visible canvases must reach the renderer");
        let canvas = h.app.session.doc.canvases.iter().find(|c| c.name == "Three").unwrap();
        let image = images.iter().find(|i| i.id == canvas.id).unwrap();
        assert_eq!(image.corners, [[-20.0, -20.0, 5.0], [20.0, -20.0, 5.0], [20.0, 20.0, 5.0], [-20.0, 20.0, 5.0]]);
    }

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

    /// A canvas of a photo stored on its side with an EXIF orientation is drawn upright (#19112): its
    /// texture is the size the photo shows at, on a quad of that shape.
    #[test]
    fn a_turned_photo_canvas_is_drawn_upright() {
        use image::ImageEncoder;
        use serde_json::json;

        let mut out = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut out);
        // A big-endian TIFF block whose IFD0 has one entry: Orientation (0x0112) = 6.
        encoder.set_exif_metadata([b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06".as_slice(), &[0; 6]].concat()).unwrap();
        encoder.write_image(&[128; 32 * 16 * 3], 32, 16, image::ExtendedColorType::Rgb8).unwrap();
        let mut h = crate::scenario::Harness::new();
        h.app.run("canvas.insert", json!({"path": crate::scenario::image_file(&out), "plane": "XY", "width": 60})).unwrap();
        let images = super::gpu_images(&h.app);
        let [image] = images.as_slice() else { panic!("one canvas: {}", images.len()) };
        assert_eq!(image.size, [16, 32], "the texture is upright");
        assert_eq!(image.corners, [[-30.0, -60.0, 0.0], [30.0, -60.0, 0.0], [30.0, 60.0, 0.0], [-30.0, 60.0, 0.0]], "60 mm wide, 120 high");

        // Another open document's canvas with this id and these bytes, saved before orientations were
        // (#19112): its pixels are as stored, not the cached turned ones.
        std::sync::Arc::make_mut(&mut h.app.session.doc).canvases[0].orientation = 1;
        let images = super::gpu_images(&h.app);
        let [image] = images.as_slice() else { panic!("one canvas: {}", images.len()) };
        assert_eq!(image.size, [32, 16], "the texture as stored");
    }
}
