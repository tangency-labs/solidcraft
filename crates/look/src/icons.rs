//! SolidCraft's icon set: the SVG masters in `icons/design/svg`, drawn by the kit's renderer
//! (`solidcraft_ui_kit::icons`).
//!
//! The masters, `palette.toml` (every colour role in light and dark, the accents and the icons'
//! default accents) and `aliases.toml` are embedded at build time (see `build.rs`) as [`DATA`],
//! which [`SET`] draws and [`install`] makes the kit's set. The masters are a small, linted SVG
//! subset (`icons/design/tools/lint_icons.py`); a master never carries a colour of its own: it
//! names a palette role. `palette.toml` owns defaults; theme files may override display roles and
//! semantic ramps.
//!
//! A host that draws these icons plus a few masters of its own installs
//! `IconSet::extended(DATA, extension)` instead of [`SET`]: the same tables, never a copy.

use solidcraft_ui_kit::icons::{self, IconData, IconSet};

// The compiled tables: `ROLE_NAMES`, `ROLE_SOURCE_HEX`, `PALETTE`, `ACCENT_NAMES`, `ACCENTS`,
// `ACCENT_VARIANTS`, `MASTERS`, `ALIASES` and `ICON_ACCENTS`, as `IconData` names them.
include!(concat!(env!("OUT_DIR"), "/icon_data.rs"));

const _: () = assert!(
    ROLE_NAMES.len() <= icons::MAX_ROLES && ACCENT_NAMES.len() <= icons::MAX_ACCENTS,
    "a theme file must be able to override every role and accent"
);

/// SolidCraft's icon artwork as data, for [`SET`] or a host's `IconSet::extended` over it.
pub const DATA: IconData = IconData {
    role_names: ROLE_NAMES,
    role_source_hex: ROLE_SOURCE_HEX,
    palette: PALETTE,
    accent_names: ACCENT_NAMES,
    accents: ACCENTS,
    accent_variants: ACCENT_VARIANTS,
    masters: MASTERS,
    aliases: ALIASES,
    icon_accents: ICON_ACCENTS,
    danger_accent: "constraint",
};

/// SolidCraft's icons.
pub static SET: IconSet = IconSet::new(DATA);

/// Make [`SET`] the icons the kit draws, once per process, before the first frame and in every test
/// context; calling it again does nothing.
pub fn install() {
    icons::install(&SET);
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_ui_kit::icons::{Doc, Look, PLACEHOLDER, Resolved, render, render_tinted};
    use tiny_skia as sk;

    // SolidCraft's set whether or not it is installed.
    fn resolve(name: &str) -> Resolved {
        SET.resolve(name)
    }

    fn doc_for(master: &str, px: u32) -> Option<&'static Doc> {
        SET.doc_for(master, px)
    }

    fn accent_index(name: &str) -> Option<usize> {
        SET.accent_index(name)
    }

    fn primary(name: &str, dark: bool, vision: solidcraft_theme::ColorVision) -> egui::Color32 {
        SET.primary(name, dark, vision)
    }

    fn role_colour(name: &str, look: Look) -> egui::Color32 {
        SET.role_colour(name, look)
    }

    #[test]
    fn every_master_parses() {
        for (id, fam, src) in MASTERS {
            if let Err(e) = SET.parse(src) {
                panic!("{fam}/{id}: {e}");
            }
        }
        assert!(SET.has_master(PLACEHOLDER), "the placeholder master exists");
    }

    #[test]
    fn aliases_point_at_masters_and_known_accents() {
        for (alias, master, accent) in ALIASES {
            assert!(SET.has_master(master), "{alias} -> {master}");
            assert!(accent.is_empty() || ACCENT_NAMES.contains(accent), "{alias}: accent {accent}");
            assert!(!resolve(alias).placeholder);
        }
    }

    #[test]
    fn knockout_fills_and_strokes_erase_earlier_layers_and_allow_later_foreground() {
        let doc = SET.parse("<svg><path d='M0 0L64 0L64 64L0 64Z' fill='#8e9faf'/><circle cx='20' cy='20' r='10' fill='#ffffff'/><circle cx='20' cy='20' r='3' fill='#236ead'/><path d='M8 44L56 44' stroke='#ffffff' stroke-width='6'/><circle cx='44' cy='20' r='6' fill='#ffffff' opacity='0.5'/></svg>").unwrap();
        for dark in [false, true] {
            for accent in std::iter::once(None).chain((0..ACCENT_NAMES.len()).map(Some)) {
                let look = Look { dark, accent, vision: solidcraft_theme::ColorVision::Off, palette: None };
                let pm = render(&doc, look, 64).unwrap();
                assert_eq!(pm.pixel(26, 20).unwrap().alpha(), 0, "fill must reveal the real background");
                assert_eq!(pm.pixel(20, 44).unwrap().alpha(), 0, "stroke must reveal the real background");
                assert_eq!(pm.pixel(20, 20).unwrap().alpha(), 255, "foreground is drawn after erasure");
                assert_eq!(pm.pixel(2, 2).unwrap().alpha(), 255, "unaffected base stays opaque");
                assert!((126..=128).contains(&pm.pixel(44, 20).unwrap().alpha()), "partial opacity erases partially");
            }
        }
    }

    #[test]
    fn knockout_respects_its_clip_and_combines_explicit_masks_with_clips() {
        let doc = SET.parse("<svg><defs><clipPath id='left'><path d='M0 0L32 0L32 64L0 64Z'/></clipPath><mask id='all'><path d='M0 0L64 0L64 64L0 64Z' fill='white'/></mask></defs><path d='M0 0L64 0L64 64L0 64Z' fill='#8e9faf'/><g clip-path='url(#left)'><circle cx='32' cy='32' r='16' fill='#ffffff' mask='url(#all)'/></g></svg>").unwrap();
        let pm = render(&doc, Look { dark: false, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None }, 64).unwrap();
        assert_eq!(pm.pixel(24, 32).unwrap().alpha(), 0);
        assert_eq!(pm.pixel(40, 32).unwrap().alpha(), 255, "cutout must not escape its parent clip");
    }

    #[test]
    fn accepted_badges_and_environment_sun_have_transparent_holes() {
        for dark in [false, true] {
            let look = Look { dark, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None };
            assert_eq!(role_colour("gap", look), egui::Color32::TRANSPARENT);
            for (id, x, y) in [("contact_new", 48, 48), ("environment_map", 45, 19), ("line_mid", 35, 28)] {
                let pm = render(doc_for(id, 64).unwrap(), look, 64).unwrap();
                assert_eq!(pm.pixel(x, y).unwrap().alpha(), 0, "{id}: GAP must not be a painted theme colour");
            }
        }
    }

    #[test]
    fn native_clip_paths_and_plus_mask_keep_the_artwork_inside_its_shape() {
        for dark in [false, true] {
            for px in [16, 24, 64] {
                let look = Look { dark, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None };
                for id in ["environment_map", "curvature_map", "iso_analysis", "zebra"] {
                    let doc = doc_for(id, px).unwrap();
                    let pm = render(doc, look, px).unwrap();
                    assert_eq!(pm.pixel(px - 2, px - 2).unwrap().alpha(), 0, "{id} escapes its native clip");
                    assert!(pm.pixel(px / 2, px / 2).unwrap().alpha() > 0, "{id} lost its clipped contents");
                }
            }
            let pm = render(doc_for("new", 64).unwrap(), Look { dark, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None }, 64)
                .unwrap();
            assert_eq!(pm.pixel(45, 45).unwrap().alpha(), 0, "the plus is genuinely punched out in the native renderer");
        }
    }

    /// The token crate writes the primary strokes it shares with the artwork in code; they stay `palette.toml`'s
    /// `active-3` values, including the colour vision variants.
    #[test]
    fn tokens_share_the_artworks_primary_strokes() {
        use solidcraft_theme::icon_strokes as tokens;
        let role = ROLE_NAMES.iter().position(|r| *r == "active-3").unwrap();
        let primary = |accent: &str| {
            let items = ACCENTS[accent_index(accent).unwrap()];
            items.iter().find(|(r, _, _)| *r == role).map(|(_, light, dark)| [*light, *dark]).unwrap()
        };
        let variant = |vision: &str, accent: &str| {
            let a = accent_index(accent).unwrap();
            ACCENT_VARIANTS
                .iter()
                .find(|(v, x, r, _, _)| *v == vision && *x == a && *r == role)
                .map(|(_, _, _, light, dark)| [*light, *dark])
                .unwrap()
        };
        assert_eq!(tokens::SKETCH_PRIMARY, primary("sketch"));
        assert_eq!(tokens::CONSTRAINT_PRIMARY, primary("constraint"));
        assert_eq!(tokens::CONSTRUCTION_PRIMARY, primary("construction"));
        assert_eq!(tokens::SKETCH_RED_GREEN, variant("red-green", "sketch"));
        assert_eq!(tokens::CONSTRUCTION_RED_GREEN, variant("red-green", "construction"));
        assert_eq!(tokens::CONSTRAINT_RED_GREEN, variant("red-green", "constraint"));
        assert_eq!(tokens::CONSTRUCTION_BLUE_YELLOW, variant("blue-yellow", "construction"));
        assert_eq!(tokens::CONSTRAINT_BLUE_YELLOW, variant("blue-yellow", "constraint"));
    }

    #[test]
    fn semantic_ui_tokens_and_icon_strokes_share_the_palette_including_accessibility() {
        for dark in [false, true] {
            for vision in solidcraft_theme::ColorVision::ALL {
                let tokens = solidcraft_theme::Tokens::themed(dark, vision);
                assert_eq!(primary("line", dark, vision), tokens.sketch_accent);
                assert_eq!(primary("c_parallel", dark, vision), tokens.glyph_text);
                assert_eq!(primary("centerline", dark, vision), tokens.construction_geom);
                assert_eq!(primary("construction", dark, vision), tokens.construction_geom);
            }
        }
    }

    #[test]
    fn semantic_ramps_preserve_silhouettes_and_passive_faces_in_all_themes() {
        use solidcraft_theme::ColorVision;
        for id in ["line", "c_parallel", "construction", "centerline", "sketch_rect_pattern", "surface_extrude", "mesh_export"] {
            let resolved = resolve(id);
            for dark in [false, true] {
                for vision in ColorVision::ALL {
                    let look = Look { dark, accent: resolved.accent, vision, palette: None };
                    let base = Look { accent: None, ..look };
                    for role in ["ink", "line", "passive-1", "passive-2", "passive-3", "gap"] {
                        let role = ROLE_NAMES.iter().position(|name| *name == role).unwrap();
                        assert_eq!(SET.role_rgb(look, role), SET.role_rgb(base, role), "{id} changes passive paint");
                    }
                    for px in [16, 24, 64] {
                        let doc = doc_for(resolved.master, px).unwrap();
                        let changed = render(doc, look, px).unwrap();
                        let original = render(doc, base, px).unwrap();
                        assert!(changed.pixels().iter().zip(original.pixels()).all(|(a, b)| a.alpha() == b.alpha()), "{id}: silhouette at {px}px");
                    }
                }
            }
        }
    }

    #[test]
    fn helper_controls_resolve_without_command_registry_entries() {
        for name in ["agent_chat", "normal", "infinite", "pattern_flip", "pattern_center"] {
            assert!(!resolve(name).placeholder, "UI helper: {name}");
        }
    }

    #[test]
    fn reviewed_model_folder_is_also_the_folder_drawn_in_the_app() {
        let source = MASTERS.iter().find(|(id, fam, _)| *id == "folder" && *fam == "model").unwrap().2;
        let expected = SET.parse(source).unwrap();
        for dark in [false, true] {
            let look = Look { dark, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None };
            let actual = render(doc_for("folder", 64).unwrap(), look, 64).unwrap();
            let reviewed = render(&expected, look, 64).unwrap();
            assert_eq!(actual.data(), reviewed.data());
        }
    }

    #[test]
    fn unknown_names_draw_the_placeholder() {
        assert!(resolve("no_such_icon").placeholder);
        assert!(resolve("").placeholder);
        assert!(!resolve("extrude").placeholder);
    }

    fn coverage(pm: &sk::Pixmap) -> usize {
        pm.pixels().iter().filter(|p| p.alpha() > 0).count()
    }

    #[test]
    fn every_master_draws_something_at_every_size_in_both_themes() {
        for id in SET.master_ids() {
            for dark in [false, true] {
                for px in [16, 20, 26, 64] {
                    let Some(doc) = doc_for(id, px) else { panic!("{id}") };
                    let pm = render(doc, Look { dark, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None }, px)
                        .unwrap_or_else(|| panic!("{id} at {px}"));
                    assert!(coverage(&pm) > 8, "{id} at {px}px (dark {dark}) is blank");
                }
            }
        }
    }

    fn rgba(pm: &sk::Pixmap) -> Vec<u8> {
        pm.data().to_vec()
    }

    #[test]
    fn dark_theme_and_accents_change_colour_not_shape() {
        let doc = doc_for("extrude", 64).unwrap_or_else(|| panic!("extrude"));
        let light = render(doc, Look { dark: false, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None }, 64)
            .unwrap_or_else(|| panic!("render"));
        let dark = render(doc, Look { dark: true, accent: None, vision: solidcraft_theme::ColorVision::Off, palette: None }, 64)
            .unwrap_or_else(|| panic!("render"));
        assert_ne!(rgba(&light), rgba(&dark));
        let r = resolve("surface_extrude");
        assert_eq!(r.master, "extrude");
        let surface = render(doc, Look { dark: false, accent: r.accent, vision: solidcraft_theme::ColorVision::Off, palette: None }, 64)
            .unwrap_or_else(|| panic!("render"));
        assert!(r.accent.is_some());
        assert_ne!(rgba(&light), rgba(&surface));
        // Same silhouette: the alpha channel is identical.
        let alpha = |pm: &sk::Pixmap| pm.pixels().iter().map(|p| p.alpha()).collect::<Vec<_>>();
        assert_eq!(alpha(&light), alpha(&surface));
    }

    #[test]
    fn palette_roles_are_distinct_and_complete() {
        assert_eq!(ROLE_NAMES.len(), ROLE_SOURCE_HEX.len());
        let mut seen = std::collections::HashSet::new();
        assert!(
            ROLE_NAMES.iter().zip(ROLE_SOURCE_HEX).filter(|(n, _)| **n != "line").all(|(_, h)| seen.insert(*h)),
            "two roles share a source identifier"
        );
        assert!(ROLE_NAMES.contains(&"line"));
        assert_eq!(PALETTE.len(), 2);
        assert!(PALETTE.iter().all(|p| p.len() == ROLE_NAMES.len()));
    }

    #[test]
    fn danger_tint_preserves_faces_outlines_and_transparent_details() {
        for dark in [false, true] {
            for vision in solidcraft_theme::ColorVision::ALL {
                let look = Look { dark, accent: accent_index("constraint"), vision, palette: None };
                for id in ["delete", "new", "extrude"] {
                    for px in [16, 20, 64] {
                        let doc = doc_for(id, px).unwrap();
                        let normal = render(doc, look, px).unwrap();
                        let tinted = render_tinted(doc, look, px).unwrap();
                        assert!(
                            normal.pixels().iter().zip(tinted.pixels()).all(|(a, b)| a.alpha() == b.alpha()),
                            "{id}/{px}: preserve contours, handle, slat cutouts and masks"
                        );
                        if id == "delete" && px == 64 {
                            let image = egui::ColorImage::from_rgba_premultiplied([64, 64], tinted.data());
                            for role in ["ink", "active-2", "active-3"] {
                                assert!(
                                    image.pixels.contains(&role_colour(role, look)),
                                    "{role}, dark={dark}/{vision:?}: retain separate face and outline colours"
                                );
                            }
                            for role in ["passive-2", "passive-3"] {
                                assert!(!image.pixels.contains(&role_colour(role, look)), "{role}: bin faces must take the red ramp");
                            }
                        }
                    }
                }
            }
        }
    }
}
