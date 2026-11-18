//! SolidCraft's icon artwork. Compiled only at L5.
//!
//! Command, menu, browser and timeline icons are SolidCraft's SVG masters (`solidcraft_look::icons::SET`),
//! drawn by the kit's painters ([`crate::ui_kit::icons`], re-exported here): colours come from the
//! default `palette.toml` roles/accents and optional active-theme overrides, never arbitrary caller
//! tints. An icon with no master yet draws the placeholder.
//! The marker and navigation cube artwork below is not command icons; SolidCraft's code-drawn
//! glyphs (`solidcraft_look::glyphs`) are installed with its look.
pub use crate::ui_kit::icons::{
    clear_texture_cache, paint_accented, paint_app_bar, paint_danger, paint_icon, paint_ink, paint_ink_flipped, paint_ink_mirrored, paint_on,
};

pub(crate) mod markers;
pub(crate) mod navigation;

pub use crate::ui_kit::icons::wave::{Wave, WaveSet, paint_ribbon_block, paint_ribbon_icon, set_wave, wave_flags};

/// How strongly the wave shows over a panel.
const WAVE_PANEL_ALPHA: f32 = 0.22;

/// The brand mark, in the RGB wave's faces while the wave is on.
pub(crate) fn brand_mark(painter: &egui::Painter, at: egui::Pos2) {
    match wave_logo(painter.ctx(), at.x) {
        Some(faces) => solidcraft_look::glyphs::brand_mark_coloured(painter, at, faces),
        None => solidcraft_look::glyphs::brand_mark(painter, at),
    }
}

/// The logo's faces while the wave is on: the wave's colour at `x` in its three shades.
pub(crate) fn wave_logo(ctx: &egui::Context, x: f32) -> Option<[egui::Color32; 3]> {
    let set = wave_flags(ctx);
    set.on.then(|| solidcraft_theme::Tokens::logo_shades(set.wave.tint(x, ctx.input(|i| i.time), 1.0)))
}

/// One colour cycle of the wave, to repeat across a panel.
fn wave_texture(ctx: &egui::Context) -> egui::TextureId {
    let id = egui::Id::unique("sc_rgb_wave_texture");
    if let Some(tex) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return tex.id();
    }
    let pixels = (0..256).map(Wave::cycle_colour).collect();
    let image = egui::ColorImage { size: [256, 1], source_size: egui::vec2(256.0, 1.0), pixels };
    let options = egui::TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..egui::TextureOptions::LINEAR };
    let tex = ctx.load_texture("rgb-wave", image, options);
    let handle = tex.id();
    ctx.data_mut(|d| d.insert_temp(id, tex));
    handle
}

/// With the wave on and its panel target chosen: the wave over the frame's panel fills, as a
/// gradient by screen position that keeps each panel's rounded corners. Before glass, which finds the
/// fills in these shapes.
pub(crate) fn wave_panels(ctx: &egui::Context) {
    use egui::epaint::{RectShape, Shape};
    let set = wave_flags(ctx);
    if !(set.on && set.panels) {
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(33));
    let t = solidcraft_theme::Tokens::get();
    let hosts = [t.panel, t.panel_header, t.toolbar, t.timeline, t.dialog_bg, t.dialog_body, t.app_bar, t.overlay];
    let wave = set.wave;
    let phase = wave.phase(ctx.input(|i| i.time));
    let texture = wave_texture(ctx);
    let screen = ctx.content_rect();
    let mut layers = vec![egui::LayerId::background()];
    layers.extend(ctx.memory(|m| m.layer_ids().filter(|l| *l != egui::LayerId::background()).collect::<Vec<_>>()));
    let over = |r: &RectShape| {
        let u = |x: f32| x / wave.length - phase;
        let uv = egui::Rect::from_min_max(egui::pos2(u(r.rect.left()), 0.0), egui::pos2(u(r.rect.right()), 1.0));
        let tint = egui::Color32::WHITE.gamma_multiply(WAVE_PANEL_ALPHA);
        Shape::Rect(RectShape::filled(r.rect, r.corner_radius, tint).with_texture(texture, uv))
    };
    ctx.graphics_mut(|g| {
        for layer in layers {
            let Some(list) = g.get_mut(layer) else { continue };
            let mut done: Vec<egui::Rect> = Vec::new();
            for i in 0..list.next_idx().0 {
                list.mutate_shape(egui::layers::ShapeIdx(i), |clipped| {
                    let mut wanted = |r: &RectShape| {
                        // Once for what is nested: a header inside its panel doesn't deepen it.
                        let take = hosts.contains(&r.fill)
                            && r.rect.width() >= 24.0
                            && r.rect.height() >= 20.0
                            && r.rect.area() <= screen.area() * 0.6
                            && !done.iter().any(|d| d.contains_rect(r.rect));
                        if take {
                            done.push(r.rect);
                        }
                        take
                    };
                    match &mut clipped.shape {
                        Shape::Rect(r) if wanted(r) => {
                            let tinted = over(r);
                            let fill = Shape::Rect(r.clone());
                            clipped.shape = Shape::Vec(vec![fill, tinted]);
                        }
                        Shape::Vec(parts) => {
                            let found = parts.iter().position(|p| matches!(p, Shape::Rect(r) if hosts.contains(&r.fill)));
                            if let Some(j) = found
                                && let Shape::Rect(r) = &parts[j]
                                && wanted(r)
                            {
                                let tinted = over(r);
                                parts.insert(j + 1, tinted);
                            }
                        }
                        _ => {}
                    }
                });
            }
        }
    });
}

#[cfg(test)]
mod wave_tests {
    use super::*;

    /// The wave's tint on a ribbon icon follows screen x and time, and is the artwork's own when off.
    #[test]
    fn rgb_wave_tint_follows_screen_x_and_time_and_off_is_normal_artwork() {
        let ctx = crate::fonts::test_context();
        let wave = Wave { length: 600.0, speed: 150.0 };
        let tints = |on: bool, time: f64| {
            let input = egui::RawInput { time: Some(time), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                set_wave(ui.ctx(), WaveSet { on, icons: on, wave, ..Default::default() });
                let at = |x: f32, y: f32| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(32.0, 32.0));
                paint_ribbon_icon(ui.painter(), at(10.0, 0.0), "extrude", 1.0);
                paint_ribbon_icon(ui.painter(), at(310.0, 0.0), "extrude", 1.0);
                paint_ribbon_icon(ui.painter(), at(10.0, 90.0), "save", 1.0);
            });
            let colours = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::epaint::Shape::Mesh(m) => m.vertices.first().map(|v| v.color),
                    _ => None,
                })
                // With the wave on, each icon is its neutral part and its tinted accent.
                .filter(|c| !on || *c != egui::Color32::WHITE)
                .collect::<Vec<_>>();
            out.textures_delta.clear();
            colours
        };
        let off = tints(false, 0.0);
        assert!(off.len() >= 3 && off.iter().all(|c| *c == egui::Color32::WHITE), "off: plain artwork {off:?}");
        let (a, b) = (tints(true, 0.0), tints(true, 1.0));
        assert!(a.len() >= 3 && a[0] != a[1], "one icon at two x values differs");
        assert_eq!(a[0], a[2], "two icons at one x agree");
        assert_ne!(a[0], b[0], "time moves the wave");
        assert_eq!(wave.tint(10.0, 0.0, 1.0), wave.tint(10.0 + 600.0, 0.0, 1.0), "one cycle per length");
        assert_eq!(wave.tint(10.0, 2.0, 1.0), wave.tint(310.0, 0.0, 1.0), "it travels at its speed");
        assert!(wave.tint(10.0, 0.0, 0.35).a() < 120, "a disabled icon stays dim");
    }

    #[test]
    fn the_logo_follows_the_wave_only_while_it_is_on() {
        let ctx = egui::Context::default();
        let wave = Wave { length: 600.0, speed: 150.0 };
        set_wave(&ctx, WaveSet { wave, ..Default::default() });
        assert_eq!(wave_logo(&ctx, 10.0), None, "off: the logo keeps its own colours");
        set_wave(&ctx, WaveSet { on: true, wave, ..Default::default() });
        assert_eq!(wave_logo(&ctx, 10.0).map(|faces| faces[1]), Some(wave.tint(10.0, ctx.input(|i| i.time), 1.0)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icon_svg::{self, Look};
    use crate::theme;
    use egui::{Color32, Rect};
    use solidcraft_theme::ColorVision;
    use std::collections::HashMap;

    #[test]
    fn theme_icon_palettes_change_uploaded_pixels_and_reuse_only_matching_textures() {
        use serde_json::json;
        let ctx = crate::fonts::test_context();
        let dark = solidcraft_theme::is_dark();
        ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
        let mut previous_alpha = None;
        let mut previous_texture = None;
        for (stroke, expected) in [
            ("#994477", Color32::from_rgb(0x99, 0x44, 0x77)),
            ("#226699", Color32::from_rgb(0x22, 0x66, 0x99)),
            ("#994477", Color32::from_rgb(0x99, 0x44, 0x77)),
        ] {
            let file = crate::theme_files::from_value(&json!({"format":crate::theme_files::FORMAT,
                "name":"Switch","base":if dark {"dark"} else {"light"},
                "icon_palette":{"solid":["#c0d0e0","#668899",stroke]}}))
            .unwrap();
            let _restore = theme::scoped_file(&file);
            let draw = |ui: &mut egui::Ui| {
                let r = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0));
                paint_icon(ui.painter(), r, "extrude", 1.0);
                // A forced-dark monochrome app-bar icon must not evict the theme palette.
                paint_app_bar(ui.painter(), r.translate(egui::vec2(70.0, 0.0)), "save", 1.0);
            };
            let mut out = ctx.run_ui(egui::RawInput::default(), draw);
            let (id, image) = out
                .textures_delta
                .set
                .iter()
                .flat_map(|(id, deltas)| deltas.iter().map(move |d| (id, &d.image)))
                .find_map(|(id, data)| match data {
                    egui::ImageData::Color(image) if image.size == [64, 64] && image.pixels.contains(&expected) => Some((*id, image)),
                    _ => None,
                })
                .expect("the new theme's actual SVG texture");
            let alpha: Vec<_> = image.pixels.iter().map(|p| p.a()).collect();
            if let Some(old) = &previous_alpha {
                assert_eq!(&alpha, old, "theme colours preserve artwork and cutouts");
            }
            if let Some(old) = previous_texture {
                assert_ne!(id, old, "switching a same-base theme uploads a new texture");
            }
            previous_alpha = Some(alpha);
            previous_texture = Some(id);
            out.textures_delta.clear();
            let mut steady = ctx.run_ui(egui::RawInput::default(), draw);
            assert!(
                steady.textures_delta.set.values().flatten().all(|d| match &d.image {
                    egui::ImageData::Color(i) => i.size != [64, 64],
                }),
                "unchanged palette reuses cached textures"
            );
            steady.textures_delta.clear();
        }
    }

    #[test]
    fn assistant_pixels_use_the_application_bar_text_token() {
        let previous = solidcraft_theme::is_dark();
        for dark in [false, true] {
            let ctx = crate::fonts::test_context();
            solidcraft_theme::set_dark(dark);
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint_app_bar(ui.painter(), Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(32.0, 32.0)), "assistant", 1.0);
            });
            let ink = solidcraft_theme::Tokens::get().app_bar_text;
            assert!(out.textures_delta.set.iter().any(|(_, deltas)| deltas.iter().any(|delta| match &delta.image {
                egui::ImageData::Color(image) => image.pixels.contains(&ink),
            })));
            out.textures_delta.clear();
        }
        solidcraft_theme::set_dark(previous);
    }

    #[test]
    fn actual_uploaded_icon_pixels_follow_semantics_and_vision_cache_changes() {
        let ctx = crate::fonts::test_context();
        let previous = solidcraft_theme::vision();
        for dark in [false, true] {
            for vision in [ColorVision::Off, ColorVision::Protan, ColorVision::Tritan] {
                solidcraft_theme::set_vision(vision);
                // The line types: Normal in the base blue, and Construction, Centerline and Infinite
                // in the construction accent, as the sketch canvas draws them.
                for icon in
                    ["line", "c_parallel", "normal", "construction", "centerline", "infinite", "surface_extrude", "mesh_export", "contact_new"]
                {
                    let expected = icon_svg::primary(icon, dark, vision);
                    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                        let r = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0));
                        paint_on(ui.painter(), r, icon, Some(dark), 1.0);
                    });
                    assert!(
                        output.textures_delta.set.iter().any(|(_, deltas)| deltas.iter().any(|delta| match &delta.image {
                            egui::ImageData::Color(image) => image.pixels.contains(&expected),
                        })),
                        "{icon}, {dark}, {vision:?}: actual texture misses its semantic stroke or reused a stale cache entry"
                    );
                    if icon == "contact_new" {
                        let image = output
                            .textures_delta
                            .set
                            .values()
                            .flatten()
                            .find_map(|delta| match &delta.image {
                                egui::ImageData::Color(image) if image.size == [64, 64] => Some(image),
                                _ => None,
                            })
                            .unwrap();
                        assert_eq!(image.pixels.get(48 * 64 + 48), Some(&Color32::TRANSPARENT), "uploaded plus must stay transparent");
                    }
                    // This headless test inspects uploads instead of sending them
                    // to a GPU; acknowledge the deltas before they are dropped.
                    output.textures_delta.clear();
                }
            }
        }
        solidcraft_theme::set_vision(previous);
    }

    #[test]
    fn per_call_accents_preserve_roles_and_share_the_existing_cache() {
        let previous = solidcraft_theme::vision();
        for dark in [false, true] {
            for vision in ColorVision::ALL {
                solidcraft_theme::set_vision(vision);
                let ctx = crate::fonts::test_context();
                ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
                let mut textures = HashMap::new();
                let mut ids = HashMap::new();
                let mut silhouette = None;
                for accent in [
                    Some("construction"),
                    Some("constraint"),
                    None,
                    Some("sketch"),
                    Some("surface"),
                    Some("form"),
                    Some("mesh"),
                    Some("unknown"),
                    Some(""),
                    None,
                ] {
                    let expected_accent = match accent {
                        Some("unknown" | "") => icon_svg::resolve("surface_extrude").accent,
                        Some(name) => icon_svg::accent_index(name),
                        None => None,
                    };
                    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                        paint_accented(ui.painter(), Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)), "surface_extrude", accent, 0.42);
                    });
                    let texture = output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) => {
                                assert!(mesh.vertices.iter().all(|vertex| vertex.color == Color32::from_white_alpha(107)));
                                Some(mesh.texture_id)
                            }
                            _ => None,
                        })
                        .unwrap();
                    for (id, deltas) in &output.textures_delta.set {
                        for delta in deltas {
                            let egui::ImageData::Color(image) = &delta.image;
                            if delta.pos.is_none() {
                                textures.insert(*id, image.clone());
                            }
                        }
                    }
                    if let Some(previous_id) = ids.insert(expected_accent, texture) {
                        assert_eq!(texture, previous_id, "repeat and unknown-name fallback reuse the same accent texture");
                        assert!(!output.textures_delta.set.contains_key(&texture));
                    }
                    let image = textures.get(&texture).unwrap();
                    let look = Look::themed(dark, expected_accent, vision);
                    for role in ["ink", "passive-2", "passive-3", "active-1", "active-2", "active-3"] {
                        assert!(image.pixels.contains(&icon_svg::role_colour(role, look)), "{accent:?}/{role}, {dark}/{vision:?}");
                    }
                    let alpha: Vec<_> = image.pixels.iter().map(|pixel| pixel.a()).collect();
                    if let Some(previous) = &silhouette {
                        assert_eq!(&alpha, previous, "a ramp override must preserve all geometry and knockouts");
                    } else {
                        silhouette = Some(alpha);
                    }
                    output.textures_delta.clear();
                }
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    paint_icon(ui.painter(), Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 64.0)), "surface_extrude", 1.0);
                });
                let texture = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) => Some(mesh.texture_id),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(
                    ids.get(&icon_svg::resolve("surface_extrude").accent),
                    Some(&texture),
                    "the old painter retains semantic selection and shares the cache"
                );
                output.textures_delta.clear();
            }
        }
        solidcraft_theme::set_vision(previous);
    }

    #[test]
    fn danger_and_accent_only_textures_do_not_share_a_cache_entry() {
        for dark in [false, true] {
            let ctx = crate::fonts::test_context();
            ctx.set_visuals(if dark { egui::Visuals::dark() } else { egui::Visuals::light() });
            let mut textures = HashMap::new();
            for danger in [false, true, false, true] {
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(20.0, 20.0));
                    if danger {
                        paint_danger(ui.painter(), rect, "delete", 1.0);
                    } else {
                        paint_accented(ui.painter(), rect, "delete", Some("constraint"), 1.0);
                    }
                });
                let id = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Mesh(mesh) => Some(mesh.texture_id),
                        _ => None,
                    })
                    .unwrap();
                if let Some(previous) = textures.insert(danger, id) {
                    assert_eq!(id, previous, "repeated danger painting reuses its texture");
                    assert!(!output.textures_delta.set.contains_key(&id));
                }
                output.textures_delta.clear();
            }
            assert_ne!(textures[&false], textures[&true], "matching Look values must not mix ordinary gray bins with shaded red bins");
        }
    }
}
