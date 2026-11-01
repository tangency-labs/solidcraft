use super::*;
use crate::ui_kit::{
    self,
    test_support::{frame, key, pointer},
};
use egui::Shape;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn checkbox_keyboard_pointer_disabled_and_mixed() {
    for mixed in [false, true] {
        for enabled in [false, true] {
            for activation in [None, Some(egui::Key::Space), Some(egui::Key::Enter)] {
                let ctx = crate::fonts::test_context();
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
        let ctx = crate::fonts::test_context();
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

fn checkbox_shapes(output: &egui::FullOutput) -> Vec<(Rect, Vec<egui::epaint::ClippedShape>)> {
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

fn surface(
    ui: &mut Ui,
    name: &str,
    app: &mut crate::SolveApp,
    editor: &mut crate::dialogs_motion::Mo,
    appearance: &mut crate::dialogs_appearance::Ap,
    inputs: &mut [crate::selection::SelInput],
) {
    ui.set_width(if name == "appearance" { 340.0 } else { 760.0 });
    match name {
        "config" => ui_kit::scope(ui, |ui| {
            let id = ui.make_persistent_id("rule_editor");
            ui.ctx().data_mut(|d| d.insert_persisted(id, true));
            crate::dialogs_motion::rows(app, ui, editor, &mut []);
        }),
        "appearance" => ui_kit::scope(ui, |ui| {
            crate::dialogs_appearance::rows(app, ui, appearance, inputs);
        }),
        "preferences" => crate::prefs::show(app, ui.ctx()),
        "menu" => {
            crate::context_menu::menu_frame(ui, |ui| crate::toolbar::selection_filter(app, ui));
        }
        _ => panic!("unknown surface {name}"),
    }
}

#[test]
fn actual_config_appearance_preferences_and_menu_boxes_and_ticks_match_at_1x_2x() {
    let previous = crate::theme::is_dark();
    let evidence = std::env::var_os("SOLIDCRAFT_CHECKBOX_EVIDENCE").map(std::path::PathBuf::from);
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for ppp in [1.0, 2.0] {
            let reference_ctx = crate::fonts::test_context();
            reference_ctx.set_pixels_per_point(ppp);
            let mut references = Vec::new();
            for on in [false, true] {
                for disabled in [false, true] {
                    let mut value = on;
                    let mut output = reference_ctx.run_ui(Default::default(), |ui| {
                        ui.add_enabled_ui(!disabled, |ui| {
                            checkbox(ui, "Reference", &mut value);
                        });
                    });
                    let (square, shapes) = checkbox_shapes(&output).remove(0);
                    references.push((square, shapes.clone(), raster(&reference_ctx, shapes, ppp, square, &HashMap::new())));
                    output.textures_delta.clear();
                }
            }
            for name in ["config", "appearance", "preferences", "menu"] {
                let ctx = crate::fonts::test_context();
                ctx.set_pixels_per_point(ppp);
                ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                let mut app = crate::SolveApp::new(Default::default(), Default::default());
                app.home.open = false;
                app.ui.dark = dark;
                app.run("parameters.add", json!({"name":"width", "expression":"10 mm"})).unwrap();
                app.run("config.configure", json!({"params":["width"]})).unwrap();
                app.run("config.rule", json!({"rule":{"id":"width_rule","name":"Width rule", "enabled":true, "condition":{"type":"always"},"effect":{"type":"drive_parameter", "parameter":"width","expression":"12 mm"}}})).unwrap();
                let mut editor = crate::dialogs_motion::Mo::Configurations { new_row: String::new(), error: None, rule_editor: Default::default() };
                let Some((crate::dialogs::Kind::Appearance(mut appearance), mut inputs)) =
                    crate::dialogs_appearance::start(&app, "appearance.assign")
                else {
                    panic!("appearance fixture");
                };
                appearance.pbr = Some(Default::default());
                appearance.local.overwrite = true;
                app.ui.pick_bodies = true;
                if name == "preferences" {
                    crate::prefs::open_at(&mut app, crate::prefs::PAGE_GRAPHICS);
                }
                let mut textures = HashMap::new();
                let mut final_output = None;
                for _ in 0..3 {
                    crate::scenario::clear_handles();
                    let mut output = ctx.run_ui(
                        egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 1300.0))), ..Default::default() },
                        |ui| surface(ui, name, &mut app, &mut editor, &mut appearance, &mut inputs),
                    );
                    textures_update(&mut textures, &output.textures_delta);
                    output.textures_delta.clear();
                    final_output = Some(output);
                }
                let output = final_output.unwrap();
                let boxes = checkbox_shapes(&output);
                assert!(boxes.len() >= if matches!(name, "config" | "appearance") { 2 } else { 1 }, "{name}: missing actual checkbox controls");
                let mut ticks = 0;
                for (square, shapes) in boxes {
                    let checked = shapes.len() > 1;
                    ticks += usize::from(checked);
                    let Shape::Rect(box_shape) = &shapes[0].shape else {
                        panic!("checkbox box");
                    };
                    let opaque_fill = if checked { Tokens::get().form_primary } else { Tokens::get().control_fill };
                    let disabled = box_shape.fill.a() < opaque_fill.a();
                    let (reference_square, reference_shapes, reference_pixels) = &references[usize::from(checked) * 2 + usize::from(disabled)];
                    // Compare the actual path, independent of a form/menu's placement.
                    if checked {
                        let (Shape::Path(actual), Shape::Path(reference)) = (&shapes[1].shape, &reference_shapes[1].shape) else {
                            panic!("tick path");
                        };
                        let normalized = |points: &[egui::Pos2], min| points.iter().map(|p| *p - min).collect::<Vec<_>>();
                        assert_eq!(
                            normalized(&actual.points, square.min),
                            normalized(&reference.points, reference_square.min),
                            "{name}: tick geometry"
                        );
                        assert_eq!(actual.stroke, reference.stroke, "{name}: tick stroke");
                    }
                    let pixels = raster(&ctx, shapes, ppp, square, &HashMap::new());
                    let max_difference = pixels.as_raw().iter().zip(reference_pixels.as_raw()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                    assert!(max_difference <= 1, "{name}: box/check pixel difference {max_difference}, dark={dark}, ppp={ppp}");
                }
                assert!(ticks > 0, "{name}: must exercise checked rows");
                if let Some(dir) = &evidence {
                    std::fs::create_dir_all(dir).unwrap();
                    let mut image = raster(&ctx, output.shapes, ppp, Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 1300.0)), &textures);
                    let background = Tokens::get().dialog_body.to_array();
                    for pixel in image.pixels_mut() {
                        let remaining = 1.0 - f32::from(pixel.0[3]) / 255.0;
                        for (dest, back) in pixel.0.iter_mut().zip(background) {
                            *dest = (f32::from(*dest) + f32::from(back) * remaining).round() as u8;
                        }
                    }
                    image.save(dir.join(format!("{name}-{}-{ppp}x.png", if dark { "dark" } else { "light" }))).unwrap();
                }
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn grid_palette_inline_and_menu_share_every_box_state() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for ppp in [1.0, 2.0] {
            for on in [false, true] {
                for state in ["idle", "hover", "focus", "disabled"] {
                    let mut reference = None;
                    for variant in ["inline", "grid", "palette", "menu"] {
                        let ctx = crate::fonts::test_context();
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
                                                    crate::context_menu::menu_frame(ui, |ui| {
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
    crate::theme::set_dark(previous);
}

#[test]
fn checkbox_accessibility_preserves_label_checked_and_mixed_metadata() {
    for mixed in [false, true] {
        let ctx = crate::fonts::test_context();
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
        assert_eq!(before.typ, egui::WidgetType::Checkbox);
        assert_eq!(before.label.as_deref(), Some("Enabled"));
        assert_eq!(before.selected, if mixed { None } else { Some(false) });
        let after = after.expect("activation publishes the changed checked state");
        assert_eq!(after.typ, egui::WidgetType::Checkbox);
        assert_eq!(after.label.as_deref(), Some("Enabled"));
        assert_eq!(after.selected, Some(true));
    }
}
