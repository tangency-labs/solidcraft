use super::*;
use crate::ui_kit::{
    self,
    test_support::{frame, key, pointer},
};
use egui::Shape;
use std::collections::HashMap;

#[test]
fn checkbox_keyboard_pointer_disabled_and_mixed() {
    for mixed in [false, true] {
        for enabled in [false, true] {
            for activation in [None, Some(egui::Key::Space), Some(egui::Key::Enter)] {
                let ctx = crate::ui_kit::test_support::context();
                let mut on = false;
                let mut changed = false;
                let target = std::cell::Cell::new(Rect::NOTHING);
                let mut draw = |ui: &mut Ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        let response = checkbox_indeterminate(ui, "Option", &mut on, mixed);
                        changed |= response.changed();
                        target.set(response.rect);
                        if activation.is_some() {
                            response.request_focus();
                        }
                    });
                };
                frame(&ctx, vec![], &mut draw);
                if let Some(activation) = activation {
                    frame(&ctx, key(activation), &mut draw);
                } else {
                    // The label is part of the target, too.
                    let at = target.get().right_center() - vec2(1.0, 0.0);
                    frame(&ctx, pointer(at, true), &mut draw);
                    frame(&ctx, pointer(at, false), &mut draw);
                }
                assert_eq!(on, enabled, "mixed={mixed}, enabled={enabled}, key={activation:?}");
                assert_eq!(changed, enabled);
            }
        }
    }
}

#[test]
fn wrapped_checkbox_keeps_full_label_and_clicks_on_its_last_line() {
    for enabled in [false, true] {
        let ctx = crate::ui_kit::test_support::context();
        let label = "A long translated checkbox label remains readable and clickable on its final line";
        let target = std::cell::Cell::new(Rect::NOTHING);
        let mut value = false;
        let mut draw = |ui: &mut Ui| {
            ui.set_width(180.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            ui.add_enabled_ui(enabled, |ui| {
                target.set(checkbox(ui, label, &mut value).rect);
            });
        };
        frame(&ctx, vec![], &mut draw);
        let rect = target.get();
        assert!(rect.height() > Tokens::get().control_height);
        assert!(rect.width() <= 180.0);
        let at = rect.left_bottom() + vec2(Tokens::get().space_24, -2.0);
        frame(&ctx, pointer(at, true), &mut draw);
        frame(&ctx, pointer(at, false), &mut draw);
        assert_eq!(value, enabled, "a wrapped checkbox preserves native disabled and click behavior");
    }
}

pub(crate) fn checkbox_shapes(output: &egui::FullOutput) -> Vec<(Rect, Vec<egui::epaint::ClippedShape>)> {
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
pub(crate) fn raster(
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

pub(crate) fn textures_update(textures: &mut HashMap<egui::TextureId, egui::ColorImage>, delta: &egui::TexturesDelta) {
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

#[test]
fn grid_palette_inline_and_menu_share_every_box_state() {
    let previous = solidcraft_theme::is_dark();
    for dark in [false, true] {
        solidcraft_theme::set_dark(dark);
        for ppp in [1.0, 2.0] {
            for on in [false, true] {
                for state in ["idle", "hover", "focus", "disabled"] {
                    let mut reference = None;
                    for variant in ["inline", "grid", "palette", "menu"] {
                        let ctx = crate::ui_kit::test_support::context();
                        ctx.set_pixels_per_point(ppp);
                        let mut at = None;
                        let mut final_output = None;
                        for pass in 0..4 {
                            let events = if state == "hover" && pass > 0 {
                                vec![egui::Event::PointerMoved(at.unwrap())]
                            } else if state == "focus" && pass == 1 {
                                key(egui::Key::Tab)
                            } else {
                                vec![]
                            };
                            let mut output = ctx.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 120.0))),
                                    events,
                                    focused: true,
                                    ..Default::default()
                                },
                                |ui| {
                                    ui_kit::scope(ui, |ui| {
                                        ui.add_enabled_ui(state != "disabled", |ui| {
                                            let mut value = on;
                                            match variant {
                                                "grid" => ui_kit::checkbox_grid(ui, &mut [("Option", &mut value)], 1),
                                                "palette" => {
                                                    ui_kit::checkbox_row(ui, "option", "Option", value);
                                                }
                                                "menu" => {
                                                    crate::ui_kit::menu::menu_frame(ui, |ui| {
                                                        checkbox(ui, "Option", &mut value);
                                                    });
                                                }
                                                _ => {
                                                    checkbox(ui, "Option", &mut value);
                                                }
                                            }
                                        });
                                    });
                                },
                            );
                            output.textures_delta.clear();
                            let boxes = checkbox_shapes(&output);
                            assert_eq!(boxes.len(), 1, "{variant}, {state}");
                            at = Some(boxes[0].0.center());
                            final_output = Some(output);
                        }
                        let output = final_output.unwrap();
                        let (square, shapes) = checkbox_shapes(&output).remove(0);
                        if state == "focus" {
                            assert!(
                                output
                                    .shapes
                                    .iter()
                                    .any(|shape| matches!(&shape.shape, Shape::Rect(r) if r.rect == square.expand(Tokens::get().space_2))),
                                "{variant}: must actually focus"
                            );
                        }
                        if state == "hover" && !on {
                            let Shape::Rect(r) = &shapes[0].shape else {
                                panic!("box");
                            };
                            assert_eq!(r.fill, Tokens::get().hover, "{variant}: must actually hover");
                        }
                        let pixels = raster(&ctx, shapes, ppp, square, &HashMap::new());
                        if let Some(reference) = &reference {
                            assert_eq!(pixels, *reference, "{variant}: {state}, on={on}, dark={dark}, ppp={ppp}");
                        } else {
                            reference = Some(pixels);
                        }
                    }
                }
            }
        }
    }
    solidcraft_theme::set_dark(previous);
}

#[test]
fn checkbox_accessibility_preserves_label_checked_and_mixed_metadata() {
    for mixed in [false, true] {
        let ctx = crate::ui_kit::test_support::context();
        ctx.enable_accesskit();
        let mut value = false;
        let mut before = None;
        let mut after = None;
        for pass in 0..4 {
            let events = match pass {
                1 => key(egui::Key::Tab),
                3 => key(egui::Key::Space),
                _ => vec![],
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 120.0))),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ui| {
                    checkbox_indeterminate(ui, "Enabled", &mut value, mixed);
                },
            );
            output.textures_delta.clear();
            for event in output.platform_output.events {
                match event {
                    egui::output::OutputEvent::FocusGained(info) => before = Some(info),
                    egui::output::OutputEvent::Clicked(info) | egui::output::OutputEvent::ValueChanged(info) => after = Some(info),
                    _ => {}
                }
            }
        }
        let before = before.expect("checkbox gains keyboard focus");
        assert_eq!(before.role, egui::Role::CheckBox);
        assert_eq!(before.label.as_deref(), Some("Enabled"));
        assert_eq!(before.selected, if mixed { None } else { Some(false) });
        let after = after.expect("activation publishes the changed checked state");
        assert_eq!(after.role, egui::Role::CheckBox);
        assert_eq!(after.label.as_deref(), Some("Enabled"));
        assert_eq!(after.selected, Some(true));
    }
}

/// Customize Toolbar's rows (#11471): the command's icon sits between the box and the label,
/// is drawn from its artwork, and a click on it toggles the row like a click on the label.
#[test]
fn checkbox_icon_draws_the_icon_between_box_and_label_and_toggles_on_it() {
    let t = Tokens::get();
    let icon = solidcraft_engine::find_command("solid.extrude").map(|c| c.icon).unwrap_or_default();
    assert!(!icon.is_empty());
    let ctx = crate::fonts::test_context();
    let viewport = Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 120.0));
    let run = |events: Vec<egui::Event>, on: &mut bool| {
        let (mut plain_width, mut row) = (0.0, Rect::NOTHING);
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), events, ..Default::default() }, |ui| {
            plain_width = checkbox(ui, "Extrude", &mut false).rect.width();
            row = checkbox_icon(ui, "Extrude", icon, on).rect;
        });
        output.textures_delta.clear();
        (output, plain_width, row)
    };
    let mut on = false;
    let (output, plain_width, row) = run(vec![], &mut on);
    assert_eq!(row.width(), plain_width + t.space_6 + t.space_16, "the icon takes its own slot");
    let slot = Rect::from_min_max(
        egui::pos2(row.left() + t.space_14, row.top()),
        egui::pos2(row.left() + t.space_14 + t.space_6 * 2.0 + t.space_16, row.bottom()),
    );
    assert!(
        output.shapes.iter().any(|clipped| matches!(&clipped.shape,
            Shape::Mesh(mesh) if mesh.texture_id != egui::TextureId::default() && slot.contains_rect(mesh.calc_bounds()))),
        "the icon's artwork is drawn between the box and the label"
    );
    let at = slot.center();
    run(pointer(at, true), &mut on);
    run(pointer(at, false), &mut on);
    assert!(on, "a click on the icon toggles the row");

    // A wrapped label leaves room for the icon: the row stays inside a narrow card.
    let (mut wrapped, mut single) = (Rect::NOTHING, Rect::NOTHING);
    let label = "A label long enough that it has to wrap in a narrow card";
    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ui| {
        ui.set_max_width(120.0);
        single = checkbox_icon(ui, "Extrude", icon, &mut false).rect;
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        wrapped = checkbox_icon(ui, label, icon, &mut false).rect;
    });
    output.textures_delta.clear();
    assert!(wrapped.width() <= 120.0, "{wrapped:?}");
    assert!(wrapped.height() > single.height(), "{wrapped:?} {single:?}");
}
