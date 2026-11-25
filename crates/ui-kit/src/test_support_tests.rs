//! The kit's test fixtures: a context with egui's default faces (the kit ships no fonts; a host
//! installs its own) and headless frames, pointer and key events and shape walks.
use super::*;
use egui::{Event, FullOutput, RawInput, Rect, Shape, Ui, vec2};
use solidcraft_theme::Tokens;
use solidcraft_ui_registry as registry;
use std::collections::HashMap;

/// The kit's own test catalogs: the messages its tests read, with the same English a host
/// gives them, plus the German the first-letter test needs. Catalogs are host data; the kit's
/// tests never rely on SolidCraft's.
static ENGLISH: solidcraft_ui_text::Catalog = solidcraft_ui_text::Catalog::new("en", "English", include_str!("test_support/catalog.json"));
static GERMAN: solidcraft_ui_text::Catalog = solidcraft_ui_text::Catalog::new("de", "Deutsch", include_str!("test_support/catalog.de.json"));
static CATALOGS: [&solidcraft_ui_text::Catalog; 2] = [&ENGLISH, &GERMAN];

/// Install the kit's test catalog (once per process) and read the system language as English.
pub fn install_catalog() {
    solidcraft_ui_text::pin_system_language();
    solidcraft_ui_text::install(&CATALOGS);
}

/// A context for the kit's tests: egui's default faces with the kit's weight families bound to
/// them, and the kit's English test catalog.
pub fn context() -> egui::Context {
    install_catalog();
    let ctx = egui::Context::default();
    type_face::install_weight_families(&ctx);
    ctx
}

/// A frame without a renderer. Preserve all caller-supplied timing and viewport options.
pub fn raw_frame(ctx: &egui::Context, input: RawInput, add: impl FnMut(&mut Ui)) -> FullOutput {
    let mut output = ctx.run_ui(input, add);
    output.textures_delta.clear();
    output
}

pub fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<Event> {
    vec![Event::PointerMoved(pos), Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() }]
}

pub fn key(key: egui::Key) -> Vec<Event> {
    vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]
}

/// Top-level text shapes only, in their original paint order.
pub fn text_shapes(output: &FullOutput) -> impl Iterator<Item = &egui::epaint::TextShape> {
    output.shapes.iter().filter_map(|clipped| match &clipped.shape {
        Shape::Text(text) => Some(text),
        _ => None,
    })
}

/// Every leaf of a shape list, in paint order.
pub fn each_shape(shape: &Shape, visit: &mut impl FnMut(&Shape)) {
    match shape {
        Shape::Vec(shapes) => shapes.iter().for_each(|shape| each_shape(shape, visit)),
        shape => visit(shape),
    }
}

pub fn checkbox_shapes(output: &egui::FullOutput) -> Vec<(Rect, Vec<egui::epaint::ClippedShape>)> {
    let t = Tokens::get();
    output
        .shapes
        .iter()
        .filter_map(|clipped| {
            let Shape::Rect(box_shape) = &clipped.shape else { return None };
            let square = box_shape.rect;
            // Scroll areas also allocate controls below the viewport. Only complete,
            // visible boxes have pixels that can be compared to a complete reference.
            if !clipped.clip_rect.contains_rect(square)
                || square.size() != vec2(t.space_14, t.space_14)
                || box_shape.corner_radius != egui::CornerRadius::same(t.radius_sm)
            {
                return None;
            }
            let mut shapes = vec![clipped.clone()];
            shapes.extend(
                output
                    .shapes
                    .iter()
                    .filter(|candidate| {
                        matches!(&candidate.shape, Shape::Path(path)
                if matches!(path.points.len(), 2 | 3) && path.stroke.width == t.space_2 && path.points.iter().all(|p| square.contains(*p)))
                    })
                    .cloned(),
            );
            Some((square, shapes))
        })
        .collect()
}

/// Small CPU rasteriser for egui's actual tessellated triangles, including the font/icon atlas.
/// This keeps screenshots and pixel comparisons deterministic without requiring a display/GPU.
pub fn raster(
    ctx: &egui::Context,
    mut shapes: Vec<egui::epaint::ClippedShape>,
    ppp: f32,
    bounds: Rect,
    textures: &HashMap<egui::TextureId, egui::ColorImage>,
) -> image::RgbaImage {
    // Compare pixel-aligned crops in local coordinates. This avoids cancellation
    // in barycentric arithmetic at different form/menu screen positions.
    let offset = -bounds.min.to_vec2();
    for clipped in &mut shapes {
        clipped.shape.translate(offset);
        clipped.clip_rect = clipped.clip_rect.translate(offset);
    }
    let bounds = bounds.translate(offset);
    let width = (bounds.width() * ppp).round() as u32;
    let height = (bounds.height() * ppp).round() as u32;
    let mut pixels = image::RgbaImage::new(width, height);
    let cross = |a: egui::Vec2, b: egui::Vec2| a.x * b.y - a.y * b.x;
    for clipped in ctx.tessellate(shapes, ppp) {
        let egui::epaint::Primitive::Mesh(mesh) = clipped.primitive else { continue };
        let clip = clipped.clip_rect.intersect(bounds);
        for triangle in mesh.indices.as_chunks::<3>().0 {
            let [a, mut b, mut c] = triangle.map(|i| mesh.vertices[i as usize]);
            let signed_area = cross(b.pos - a.pos, c.pos - a.pos);
            if signed_area < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            let area = signed_area.abs();
            if area.abs() < 1e-6 {
                continue;
            }
            let rect = Rect::from_min_max(a.pos.min(b.pos).min(c.pos), a.pos.max(b.pos).max(c.pos)).intersect(clip);
            if !rect.is_positive() {
                continue;
            }
            let min = ((rect.min - bounds.min) * ppp).floor();
            let max = ((rect.max - bounds.min) * ppp).ceil();
            for y in (min.y.max(0.0) as u32)..(max.y as u32).min(height) {
                for x in (min.x.max(0.0) as u32)..(max.x as u32).min(width) {
                    let at = bounds.min + vec2(x as f32 + 0.5, y as f32 + 0.5) / ppp;
                    if !clip.contains(at) {
                        continue;
                    }
                    let weights = [cross(b.pos - at, c.pos - at) / area, cross(c.pos - at, a.pos - at) / area, cross(a.pos - at, b.pos - at) / area];
                    // Half-open triangles avoid double blending their shared edge.
                    if weights.iter().zip([(b.pos, c.pos), (c.pos, a.pos), (a.pos, b.pos)]).any(|(weight, (a, b))| {
                        let edge = b - a;
                        *weight < 0.0 || (*weight == 0.0 && !(edge.y > 0.0 || (edge.y == 0.0 && edge.x < 0.0)))
                    }) {
                        continue;
                    }
                    let vertices = [a, b, c];
                    let mut color = [0.0; 4];
                    let mut uv = egui::Vec2::ZERO;
                    for (v, weight) in vertices.iter().zip(weights) {
                        uv += v.uv.to_vec2() * weight;
                        for (dest, channel) in color.iter_mut().zip(v.color.to_array()) {
                            *dest += f32::from(channel) * weight;
                        }
                    }
                    if let Some(texture) = textures.get(&mesh.texture_id) {
                        let tx = (uv.x * texture.size[0] as f32).floor().max(0.0) as usize;
                        let ty = (uv.y * texture.size[1] as f32).floor().max(0.0) as usize;
                        let texel = texture.pixels[ty.min(texture.size[1] - 1) * texture.size[0] + tx.min(texture.size[0] - 1)];
                        for (dest, channel) in color.iter_mut().zip(texel.to_array()) {
                            *dest *= f32::from(channel) / 255.0;
                        }
                    } else {
                        // Box/tick isolation uses the atlas's white texel, without an atlas.
                        assert_eq!(a.uv, egui::epaint::WHITE_UV);
                        assert_eq!(b.uv, egui::epaint::WHITE_UV);
                        assert_eq!(c.uv, egui::epaint::WHITE_UV);
                    }
                    let pixel = pixels.get_pixel_mut(x, y);
                    let alpha = color[3] / 255.0;
                    for (dest, channel) in pixel.0.iter_mut().zip(color) {
                        *dest = (channel + f32::from(*dest) * (1.0 - alpha)).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
    pixels
}

pub fn textures_update(textures: &mut HashMap<egui::TextureId, egui::ColorImage>, delta: &egui::TexturesDelta) {
    for (id, deltas) in &delta.set {
        for delta in deltas {
            let egui::ImageData::Color(image) = &delta.image;
            if let Some([x, y]) = delta.pos {
                let existing = textures.get_mut(id).expect("partial texture has a base");
                for row in 0..image.height() {
                    let offset = (y + row) * existing.width() + x;
                    existing.pixels[offset..offset + image.width()].copy_from_slice(&image.pixels[row * image.width()..(row + 1) * image.width()]);
                }
            } else {
                textures.insert(*id, image.as_ref().clone());
            }
        }
    }
}

/// What [`host_marker`] paints in place of a host's backing: the frame and the opacity it was
/// painted at.
pub struct HostMarker {
    pub frame: egui::Frame,
    pub opacity: f32,
}

/// A host's window background ([`container::Background`]) that marks where the backing goes.
pub fn host_marker(painter: &egui::Painter, frame: egui::Frame, content: egui::Rect, ordinary: egui::Shape) -> egui::Shape {
    egui::Shape::Vec(vec![
        egui::Shape::Callback(egui::PaintCallback {
            rect: frame.widget_rect(content),
            callback: std::sync::Arc::new(HostMarker { frame, opacity: painter.opacity() }),
        }),
        ordinary,
    ])
}

#[test]
fn render_results_shared_divider_clamps_hostile_positions_and_uses_bounded_chrome() {
    for (input, expected) in [(f32::NAN, 0.5), (f32::INFINITY, 0.5), (f32::NEG_INFINITY, 0.5), (-1e30, 0.05), (0.25, 0.25), (1e30, 0.95)] {
        assert_eq!(comparison_position(input), expected);
    }
    let ctx = context();
    let position = std::cell::Cell::new(0.5);
    let mut draw = |ui: &mut Ui| {
        let (canvas, _) = ui.allocate_exact_size(vec2(240.0, 180.0), egui::Sense::hover());
        let mut value = position.get();
        comparison_divider(ui, canvas, &mut value, "Comparison divider");
        comparison_badge(ui, canvas, "Live preview", false);
        position.set(value);
    };
    frame(&ctx, vec![], &mut draw);
    let handle = registry::rect_of("render:compare:handle").unwrap().center();
    frame(&ctx, pointer(handle, true), &mut draw);
    let end = egui::pos2(1000.0, handle.y);
    frame(&ctx, vec![egui::Event::PointerMoved(end)], &mut draw);
    assert_eq!(position.get(), 0.95);
    frame(&ctx, pointer(end, false), &mut draw);
}

fn draw_frame(ctx: &egui::Context, events: Vec<egui::Event>, mut add: impl FnMut(&mut Ui)) -> egui::FullOutput {
    registry::clear_handles();
    raw_frame(
        ctx,
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(320.0, 400.0))),
            events,
            focused: true,
            ..Default::default()
        },
        |ui| {
            ui.set_width(280.0);
            scope(ui, &mut add);
        },
    )
}

pub fn frame(ctx: &egui::Context, events: Vec<egui::Event>, add: impl FnMut(&mut Ui)) {
    draw_frame(ctx, events, add);
}

/// The cursor the frame asks the platform for: the pointer is at `pos` for the whole frame.
pub fn frame_cursor(ctx: &egui::Context, pos: egui::Pos2, add: impl FnMut(&mut Ui)) -> egui::CursorIcon {
    draw_frame(ctx, vec![egui::Event::PointerMoved(pos)], add).platform_output.cursor_icon
}
