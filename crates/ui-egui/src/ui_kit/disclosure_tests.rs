use crate::{icon_svg, scenario, theme::Tokens, ui_kit::*};
use egui::{ColorImage, Pos2, Rect, vec2};

fn assert_glyph(
    ctx: &egui::Context,
    mut output: egui::FullOutput,
    textures: &std::collections::HashMap<egui::TextureId, ColorImage>,
    at: Pos2,
    open: bool,
) {
    let t = Tokens::get();
    let size = t.form_section_icon_size;
    let px = size.round() as u32;
    // Compare the pixels actually uploaded by the widget against the specified SVG id,
    // not a helper's return value or its state. Also check the mesh uses those pixels
    // at the control's position without the former vertical flip.
    let id = if open { "chevron" } else { "chevron_right" };
    let master = icon_svg::resolve(id).master;
    let image = icon_svg::render(
        icon_svg::doc_for(master, px).unwrap(),
        icon_svg::Look { dark: true, accent: None, vision: crate::theme::vision(), palette: None },
        px,
    )
    .unwrap();
    let mut expected = ColorImage::from_rgba_premultiplied([px as usize, px as usize], image.data());
    for pixel in &mut expected.pixels {
        *pixel = t.form_dim.gamma_multiply(f32::from(pixel.a()) / 255.0);
    }
    let texture = textures
        .iter()
        .find_map(|(id, image)| (image == &expected).then_some(*id))
        .expect("widget must upload the specified disclosure SVG pixels at token size and ink");
    let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
    let bounds = Rect::from_center_size(at, vec2(size + 2.0, size + 2.0));
    assert!(
        meshes.iter().any(|clipped| {
            let egui::epaint::Primitive::Mesh(mesh) = &clipped.primitive else { return false };
            mesh.texture_id == texture && bounds.contains_rect(mesh.calc_bounds()) && mesh.vertices.iter().all(|v| (v.pos.y < at.y) == (v.uv.y < 0.5))
        }),
        "specified glyph must actually be drawn, without flipping, at {at:?}"
    );
    output.textures_delta.clear();
}

#[test]
fn section_card_and_floating_window_folds_render_down_when_open_right_when_closed() {
    let previous = crate::theme::is_dark();
    for dark in [false, true] {
        crate::theme::set_dark(dark);
        for open in [false, true] {
            for surface in ["section", "card", "window"] {
                let ctx = crate::fonts::test_context();
                ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                let viewport = Rect::from_min_size(Pos2::ZERO, vec2(700.0, 500.0));
                let mut textures = std::collections::HashMap::new();
                // Windows need measuring passes before their first visible frame.
                for pass in 0..3 {
                    let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(viewport), ..Default::default() }, |ui| {
                        ui.set_width(Tokens::get().form_card_width);
                        if surface == "section" {
                            section(ui, "section", "Advanced", open, |_| {});
                        } else if surface == "card" {
                            collapsible_header(ui, "fillet", "Fillet", &mut !open, || {});
                        } else {
                            Panel::new(
                                "Fillet",
                                egui::Id::unique("window"),
                                egui::pos2(20.0, 20.0),
                                viewport,
                                crate::frame::Side::Left,
                                egui::Frame::NONE,
                            )
                            .window(&ctx)
                            .show(ui.ctx(), |ui| collapsible_header(ui, "fillet", "Fillet", &mut !open, || {}));
                        }
                    });
                    for (id, deltas) in &output.textures_delta.set {
                        for delta in deltas {
                            let egui::ImageData::Color(image) = &delta.image;
                            if delta.pos.is_none() {
                                textures.insert(*id, image.as_ref().clone());
                            }
                        }
                    }
                    output.textures_delta.clear();
                    if pass != 2 {
                        continue;
                    }
                    let at = if surface == "section" {
                        let rect = scenario::rect_of("dialog:section:Advanced").unwrap();
                        egui::pos2(rect.left() + Tokens::get().form_section_icon_size / 2.0, rect.center().y)
                    } else {
                        scenario::handle_at("dialog:collapse").unwrap()
                    };
                    assert_glyph(&ctx, output, &textures, at, open);
                }
            }
        }
    }
    crate::theme::set_dark(previous);
}

#[test]
fn strip_fold_and_restore_share_the_chevron_texture_with_opposite_orientation() {
    for expanded in [true, false] {
        let ctx = crate::fonts::test_context();
        let mut rect = Rect::NOTHING;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            rect = strip_toggle(ui, expanded, "Shots").rect;
        });
        let icon =
            Rect::from_min_size(rect.min, vec2(Tokens::get().control_height, Tokens::get().control_height)).shrink(Tokens::get().form_icon_inset);
        let size = icon.width().round() as u32;
        let expected = icon_svg::render(
            icon_svg::doc_for(icon_svg::resolve("chevron").master, size).unwrap(),
            icon_svg::Look { dark: true, accent: None, vision: crate::theme::vision(), palette: None },
            size,
        )
        .unwrap();
        let mut expected = ColorImage::from_rgba_premultiplied([size as usize, size as usize], expected.data());
        for pixel in &mut expected.pixels {
            *pixel = Tokens::get().form_dim.gamma_multiply(f32::from(pixel.a()) / 255.0);
        }
        let mut textures = std::collections::HashMap::new();
        super::super::checks::tests::textures_update(&mut textures, &output.textures_delta);
        let texture = textures.iter().find_map(|(id, image)| (image == &expected).then_some(*id)).expect("shared chevron artwork");
        let primitives = ctx.tessellate(output.shapes.clone(), output.pixels_per_point);
        assert!(
            primitives.iter().any(|primitive| {
                let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else { return false };
                mesh.texture_id == texture && mesh.vertices.iter().all(|vertex| ((vertex.pos.y < icon.center().y) == (vertex.uv.y < 0.5)) == expanded)
            }),
            "hide points down and restore points up"
        );
        output.textures_delta.clear();
    }
}
