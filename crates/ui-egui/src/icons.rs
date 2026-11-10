//! SolidCraft's icon artwork. Compiled only at L5.
//!
//! Command, menu, browser and timeline icons are SolidCraft's SVG masters ([`crate::icon_svg::SET`]),
//! drawn by the kit's painters ([`crate::ui_kit::icons`], re-exported here): colours come from the
//! default `palette.toml` roles/accents and optional active-theme overrides, never arbitrary caller
//! tints. An icon with no master yet draws the placeholder.
//! The glyph, marker, navigation and pointer sources below are not command icons.
pub use crate::ui_kit::icons::{
    clear_texture_cache, paint_accented, paint_app_bar, paint_danger, paint_icon, paint_ink, paint_ink_flipped, paint_ink_mirrored, paint_on,
};

#[path = "../../../assets/icons/ui/source/glyphs.rs"]
pub(crate) mod glyphs;
#[path = "../../../assets/icons/ui/source/markers.rs"]
pub(crate) mod markers;
#[path = "../../../assets/icons/ui/source/navigation.rs"]
pub(crate) mod navigation;
#[path = "../../../assets/icons/ui/source/pointer.rs"]
pub(crate) mod pointer;

/// SolidCraft's code-drawn glyphs, as the kit draws them ([`crate::ui_kit::icons::glyphs`]).
pub static GLYPHS: crate::ui_kit::icons::glyphs::Glyphs = crate::ui_kit::icons::glyphs::Glyphs {
    transport: glyphs::transport_glyph,
    caption: glyphs::paint_glyph,
    badge: glyphs::badge,
    pin: glyphs::pin_icon,
    cross: glyphs::cross,
    breadcrumb: glyphs::breadcrumb,
    axis_tip: navigation::axis_tip_scaled,
    axis_radius: navigation::AXIS_RADIUS,
    pointer: pointer::paint,
    brand_mark: glyphs::brand_mark_silhouette,
};

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
