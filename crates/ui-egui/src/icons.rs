//! Stable UI rendering boundary for the icon artwork. Compiled only at L5.
//!
//! Command, menu, browser and timeline icons are the SVG masters in `assets/icons/ui/design/svg`,
//! rendered by [`crate::icon_svg`]: colours come from the default `palette.toml` roles/accents
//! and optional active-theme overrides, never arbitrary caller tints. An icon with no master
//! yet draws the placeholder.
//! The glyph, marker, navigation and pointer sources below are not command icons.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{Color32, Painter, Rect, TextureHandle, TextureOptions, pos2};

use crate::icon_svg::{self, Look};

#[path = "../../../assets/icons/ui/source/glyphs.rs"]
pub(crate) mod glyphs;
#[path = "../../../assets/icons/ui/source/markers.rs"]
pub(crate) mod markers;
#[path = "../../../assets/icons/ui/source/navigation.rs"]
pub(crate) mod navigation;
#[path = "../../../assets/icons/ui/source/pointer.rs"]
pub(crate) mod pointer;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Finish {
    Artwork,
    Ink(Color32),
    Danger,
}

/// Textures already rasterised: one per (master, look, pixel size, finish).
#[derive(Clone, Default)]
struct Cache(Arc<Mutex<CacheEntries>>);

#[derive(Default)]
struct CacheEntries {
    palette: Option<icon_svg::Palette>,
    textures: HashMap<(&'static str, Look, u32, Finish), TextureHandle>,
}

/// Draw palette-owned icon artwork with an explicit opacity (0 to 1).
pub fn paint_icon(p: &Painter, r: Rect, name: &str, opacity: f32) {
    paint_on(p, r, name, None, opacity);
}

/// Draw the icon `name` for a surface that is always dark (the application bar), whatever the
/// theme: `dark: Some(true)`. `None` follows the theme.
pub fn paint_on(p: &Painter, r: Rect, name: &str, dark: Option<bool>, opacity: f32) {
    paint_with_ink(p, r, name, dark, opacity, None, Mirror::None);
}

/// Monochrome flat controls on the application bar use its text token exactly.
pub(crate) fn paint_app_bar(p: &Painter, r: Rect, name: &str, opacity: f32) {
    paint_with_ink(p, r, name, Some(true), opacity, Some(crate::theme::Tokens::get().app_bar_text), Mirror::None);
}

/// Paint a single-colour icon in `ink`.
pub(crate) fn paint_ink(p: &Painter, r: Rect, name: &str, ink: Color32) {
    paint_with_ink(p, r, name, Some(true), 1.0, Some(ink), Mirror::None);
}

/// Paint a single-colour icon in `ink`, mirrored top to bottom (a chevron pointing up).
pub(crate) fn paint_ink_flipped(p: &Painter, r: Rect, name: &str, ink: Color32) {
    paint_with_ink(p, r, name, Some(true), 1.0, Some(ink), Mirror::Vertical);
}

/// Paint a single-colour icon mirrored left to right, reusing the original master.
pub(crate) fn paint_ink_mirrored(p: &Painter, r: Rect, name: &str, ink: Color32) {
    paint_with_ink(p, r, name, Some(true), 1.0, Some(ink), Mirror::Horizontal);
}

#[derive(Clone, Copy)]
enum Mirror {
    None,
    Vertical,
    Horizontal,
}

fn paint_with_ink(p: &Painter, r: Rect, name: &str, dark: Option<bool>, opacity: f32, ink: Option<Color32>, mirror: Mirror) {
    let ctx = p.ctx();
    let resolved = icon_svg::resolve(name);
    let look = Look::themed(dark.unwrap_or_else(|| ctx.global_style().visuals.dark_mode), resolved.accent, crate::theme::vision());
    paint_resolved(p, r, resolved, look, opacity, ink.map_or(Finish::Artwork, Finish::Ink), mirror);
}

/// Draw with a named palette ramp, preserving outlines, passive faces and transparency.
/// `None` explicitly selects base blue; an unknown name retains the icon's own accent.
/// Existing painters continue to select the icon's semantic accent.
pub fn paint_accented(p: &Painter, r: Rect, name: &str, accent: Option<&str>, opacity: f32) {
    let resolved = icon_svg::resolve(name);
    let accent = match accent {
        Some(name) => icon_svg::accent_index(name).or(resolved.accent),
        None => None,
    };
    let look = Look::themed(p.ctx().global_style().visuals.dark_mode, accent, crate::theme::vision());
    paint_resolved(p, r, resolved, look, opacity, Finish::Artwork, Mirror::None);
}

/// Shared destructive-action artwork: red shaded faces, normal outlines and cutouts.
/// Unlike a single-colour mask, this also tints a master's passive faces through
/// the constraint ramp without flattening the lid, handle or other details.
pub(crate) fn paint_danger(p: &Painter, r: Rect, name: &str, opacity: f32) {
    let resolved = icon_svg::resolve(name);
    let look = Look::themed(p.ctx().global_style().visuals.dark_mode, icon_svg::accent_index("constraint"), crate::theme::vision());
    paint_resolved(p, r, resolved, look, opacity, Finish::Danger, Mirror::None);
}

fn paint_resolved(p: &Painter, r: Rect, resolved: icon_svg::Resolved, look: Look, opacity: f32, finish: Finish, mirror: Mirror) {
    let ctx = p.ctx();
    let ppp = ctx.pixels_per_point().max(0.5);
    let side = r.width().min(r.height());
    let px = (side * ppp).round().clamp(8.0, 512.0) as u32;
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::unique("sc_icon_cache"), Cache::default).clone());
    let Ok(mut entries) = cache.0.lock() else { return };
    let palette = crate::theme::Tokens::get().icon_palette;
    if entries.palette != palette {
        entries.textures.clear();
        entries.palette = palette;
    }
    let map = &mut entries.textures;
    let key = (resolved.master, look, px, finish);
    if let std::collections::hash_map::Entry::Vacant(slot) = map.entry(key) {
        let Some(doc) = icon_svg::doc_for(resolved.master, px) else { return };
        let pm = if finish == Finish::Danger { icon_svg::render_tinted(doc, look, px) } else { icon_svg::render(doc, look, px) };
        let Some(pm) = pm else { return };
        let mut image = egui::ColorImage::from_rgba_premultiplied([px as usize, px as usize], pm.data());
        if let Finish::Ink(ink) = finish {
            for pixel in &mut image.pixels {
                *pixel = ink.gamma_multiply(f32::from(pixel.a()) / 255.0);
            }
        }
        slot.insert(ctx.load_texture(format!("icon:{}:{px}", resolved.master), image, TextureOptions::LINEAR));
    }
    let Some(tex) = map.get(&key) else { return };
    // Snap the image to whole physical pixels so it stays crisp.
    let size = px as f32 / ppp;
    let min = pos2(((r.center().x - size / 2.0) * ppp).round() / ppp, ((r.center().y - size / 2.0) * ppp).round() / ppp);
    let tint = Color32::from_white_alpha((opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    let uv = match mirror {
        Mirror::None => Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Mirror::Vertical => Rect::from_min_max(pos2(0.0, 1.0), pos2(1.0, 0.0)),
        Mirror::Horizontal => Rect::from_min_max(pos2(1.0, 0.0), pos2(0.0, 1.0)),
    };
    p.image(tex.id(), Rect::from_min_size(min, egui::vec2(size, size)), uv, tint);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{self, ColorVision};

    #[test]
    fn theme_icon_palettes_change_uploaded_pixels_and_reuse_only_matching_textures() {
        use serde_json::json;
        let ctx = crate::fonts::test_context();
        let dark = theme::is_dark();
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
            let _restore = theme::scoped_tokens(crate::theme_files::resolve(&file));
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
        let previous = theme::is_dark();
        for dark in [false, true] {
            let ctx = crate::fonts::test_context();
            theme::set_dark(dark);
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint_app_bar(ui.painter(), Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(32.0, 32.0)), "assistant", 1.0);
            });
            let ink = theme::Tokens::get().app_bar_text;
            assert!(out.textures_delta.set.iter().any(|(_, deltas)| deltas.iter().any(|delta| match &delta.image {
                egui::ImageData::Color(image) => image.pixels.contains(&ink),
            })));
            out.textures_delta.clear();
        }
        theme::set_dark(previous);
    }

    #[test]
    fn actual_uploaded_icon_pixels_follow_semantics_and_vision_cache_changes() {
        let ctx = crate::fonts::test_context();
        let previous = theme::vision();
        for dark in [false, true] {
            for vision in [ColorVision::Off, ColorVision::Protan, ColorVision::Tritan] {
                theme::set_vision(vision);
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
        theme::set_vision(previous);
    }

    #[test]
    fn per_call_accents_preserve_roles_and_share_the_existing_cache() {
        let previous = theme::vision();
        for dark in [false, true] {
            for vision in ColorVision::ALL {
                theme::set_vision(vision);
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
        theme::set_vision(previous);
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
