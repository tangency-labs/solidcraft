//! SolidCraft's interface faces: IBM Plex Sans (one variable face at 400, 500, 600 and 700) and
//! Plex Mono, bound to the kit's weight families (`solidcraft_ui_kit::type_face`) ahead of egui's
//! own faces, with egui's Hack last for the symbols the others lack.
//!
//! A host that adds faces of its own (a chosen interface font, script fallbacks) builds on
//! [`definitions`] and installs the result with [`apply`].

use std::sync::Arc;

use egui::{FontDefinitions, FontFamily};
use solidcraft_ui_kit::type_face::{BOLD_FAMILY, MEDIUM_FAMILY, SEMIBOLD_FAMILY, SYMBOL_FALLBACK, sans_families, semibold};

/// IBM Plex Sans, variable in width and weight (SIL OFL 1.1, `fonts/ibm-plex-sans/OFL.txt`).
pub const PLEX_SANS: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/ibm-plex-sans/IBMPlexSans-Variable.ttf"));
/// IBM Plex Mono Regular (SIL OFL 1.1, `fonts/ibm-plex-mono/OFL.txt`).
pub const PLEX_MONO: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/fonts/ibm-plex-mono/IBMPlexMono-Regular.ttf"));
/// The Plex Sans instances: the font name each is installed under and its weight, in the order of
/// the kit's sans families (regular, medium, semibold, bold).
pub const WEIGHTS: [(&str, f32); 4] = [("plex-sans", 400.0), (MEDIUM_FAMILY, 500.0), (SEMIBOLD_FAMILY, 600.0), (BOLD_FAMILY, 700.0)];

/// IBM Plex Sans at 400/500/600/700 and Plex Mono Regular, followed by egui's symbol/emoji
/// fallbacks. Each variable instance shares the embedded bytes and selects a real weight.
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    for ((name, weight), family) in WEIGHTS.into_iter().zip(sans_families()) {
        let mut data = egui::FontData::from_static(PLEX_SANS);
        data.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", weight)]);
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
        fonts.families.insert(family, std::iter::once(name.to_owned()).chain(fallbacks.iter().cloned()).collect());
    }
    fonts.font_data.insert("plex-mono".to_owned(), Arc::new(egui::FontData::from_static(PLEX_MONO)));
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "plex-mono".to_owned());
    with_symbol_fallback(fonts)
}

/// `fonts` with egui's built-in Hack last in every sans family. Ubuntu-Light, NotoEmoji and
/// emoji-icon-font lack common symbols such as ▲ ▼ ↑ ↓ → ⁻, which Hack has, so they no longer
/// show as empty boxes where no system font fills in (#2955). Text the other faces draw keeps them.
pub fn with_symbol_fallback(mut fonts: FontDefinitions) -> FontDefinitions {
    if fonts.font_data.contains_key(SYMBOL_FALLBACK) {
        for family in sans_families() {
            let list = fonts.families.entry(family).or_default();
            list.retain(|name| name != SYMBOL_FALLBACK);
            list.push(SYMBOL_FALLBACK.to_owned());
        }
    }
    fonts
}

/// Make `fonts` the context's faces (from the next frame) and set headings in the semibold family.
pub fn apply(ctx: &egui::Context, fonts: FontDefinitions) {
    ctx.set_fonts(fonts);
    ctx.global_style_mut(|style| {
        if let Some(heading) = style.text_styles.get_mut(&egui::TextStyle::Heading) {
            heading.family = semibold(heading.size).family;
        }
    });
}

/// [`apply`] the bundled [`definitions`] to `ctx`.
pub fn install(ctx: &egui::Context) {
    apply(ctx, definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_faces_lead_every_family_and_keep_egui_fallbacks() {
        let defaults = with_symbol_fallback(FontDefinitions::default());
        let fonts = definitions();
        for ((name, weight), family) in WEIGHTS.into_iter().zip(sans_families()) {
            let list = &fonts.families[&family];
            assert_eq!(list[0], name);
            assert_eq!(&list[1..], defaults.families[&FontFamily::Proportional]);
            let data = &fonts.font_data[name];
            assert_eq!(data.bytes(), PLEX_SANS);
            assert_eq!(data.tweak.coords.as_ref()[0].1, weight);
            assert!(data.variation_axes().iter().any(|a| a.tag == *b"wght"));
        }
        let mono = &fonts.families[&FontFamily::Monospace];
        assert_eq!(mono[0], "plex-mono");
        assert_eq!(&mono[1..], defaults.families[&FontFamily::Monospace]);
        assert_eq!(fonts.font_data["plex-mono"].bytes(), PLEX_MONO);
    }
}
