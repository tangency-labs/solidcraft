//! Written help for the ribbon tooltips (#2758): two short sentences per command, shown under the
//! tool name when the pointer rests on its button. The names stay as they are; the help is a
//! separate message `help.<command id>` in the catalogs, so a tool without help keeps its plain
//! tooltip. A tool can also show an example picture under the text (see `ILLUSTRATIONS`); animations
//! are a later step.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::i18n;
use crate::icon_svg::{self, Look};

/// The commands that have help text. Each one needs a `help.<id>` message in the catalogs.
pub const DOCUMENTED: &[&str] = &[
    "solid.chamfer",
    "solid.combine",
    "solid.extrude",
    "solid.fillet",
    "solid.hole",
    "solid.loft",
    "solid.mirror",
    "solid.pattern.circular",
    "solid.pattern.rectangular",
    "solid.revolve",
    "solid.shell",
    "solid.sweep",
];

/// The example pictures, by command id (sorted): original SVGs on the icon grid in
/// `assets/icons/ui/design/svg/help`, drawn by [`icon_svg`] in the palette of the current theme. A
/// documented command without an entry shows its text only. Nothing is saved in a design.
const ILLUSTRATIONS: &[(&str, &str)] = &[
    ("solid.combine", include_str!("../../../assets/icons/ui/design/svg/help/help_combine.svg")),
    ("solid.loft", include_str!("../../../assets/icons/ui/design/svg/help/help_loft.svg")),
    ("solid.mirror", include_str!("../../../assets/icons/ui/design/svg/help/help_mirror.svg")),
    ("solid.pattern.circular", include_str!("../../../assets/icons/ui/design/svg/help/help_pattern_circular.svg")),
    ("solid.pattern.rectangular", include_str!("../../../assets/icons/ui/design/svg/help/help_pattern_rectangular.svg")),
    ("solid.sweep", include_str!("../../../assets/icons/ui/design/svg/help/help_sweep.svg")),
];

/// The picture is shown 240 x 180 points: the band y 8 to 56 (4:3) of the 64 x 64 grid.
const PICTURE_WIDTH: f32 = 240.0;

/// Textures already rasterised: one per (command, look, pixel width).
#[derive(Clone, Default)]
struct Pictures(Arc<Mutex<HashMap<(&'static str, Look, u32), egui::TextureHandle>>>);

/// The help for `command` in the current language, when it has any.
pub fn help(command: &str) -> Option<String> {
    if !DOCUMENTED.contains(&command) {
        return None;
    }
    let key = format!("help.{command}");
    let text = i18n::tr(&key);
    (text != key).then(|| text.to_owned())
}

/// The example picture of `command` for the current theme and workspace accent, rasterised once per size.
fn picture(ctx: &egui::Context, command: &str) -> Option<egui::TextureHandle> {
    let (id, source) = ILLUSTRATIONS.iter().find(|(id, _)| *id == command)?;
    let ppp = ctx.pixels_per_point().max(0.5);
    // A multiple of 8 pixels, so that the band (one eighth to seven eighths of the height) is whole rows.
    let px = ((PICTURE_WIDTH * ppp).round().clamp(16.0, 1024.0) as u32).div_ceil(8) * 8;
    // The `active-*` roles follow the accent of the workspace tab, like the ribbon icons; the look is part of the cache key.
    let accent = crate::theme::TabAccent::current_icon_accent(ctx).and_then(icon_svg::accent_index);
    let look = Look { dark: ctx.global_style().visuals.dark_mode, accent, vision: crate::theme::vision() };
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("sc_help_pictures"), Pictures::default).clone());
    let mut map = cache.0.lock().ok()?;
    if let std::collections::hash_map::Entry::Vacant(slot) = map.entry((id, look, px)) {
        slot.insert(ctx.load_texture(format!("help:{id}:{px}"), band(source, look, px)?, egui::TextureOptions::LINEAR));
    }
    map.get(&(*id, look, px)).cloned()
}

/// The 4:3 band of the master `source` drawn `px` pixels wide.
fn band(source: &str, look: Look, px: u32) -> Option<egui::ColorImage> {
    let doc = icon_svg::Doc::parse(source).inspect_err(|e| log::warn!("help picture: {e}")).ok()?;
    let pm = icon_svg::render(&doc, look, px)?;
    let (width, rows) = (px as usize, px as usize * 3 / 4);
    let data = pm.data().get(px as usize / 8 * width * 4..)?.get(..rows * width * 4)?;
    Some(egui::ColorImage::from_rgba_premultiplied([width, rows], data))
}

/// Show the example picture of `command` when it has one.
pub fn illustration(ui: &mut egui::Ui, command: &str) {
    if let Some(texture) = picture(ui.ctx(), command) {
        let ppp = ui.ctx().pixels_per_point().max(0.5);
        ui.add_space(4.0);
        ui.image(egui::load::SizedTexture::new(texture.id(), texture.size_vec2() / ppp));
    }
}

/// Show `tip` (name, shortcut or the reason the tool is disabled) as the tooltip of `response`,
/// with the help of `command` below it when there is one.
pub fn tooltip(response: egui::Response, command: &str, tip: String) -> egui::Response {
    let help = help(command);
    response.on_hover_ui(move |ui| {
        ui.set_max_width(300.0);
        ui.label(tip);
        if let Some(help) = help {
            ui.add_space(2.0);
            ui.label(egui::RichText::new(help).weak());
            illustration(ui, command);
        }
    })
}

/// The tooltip text of a panel drop-down row: the reason when the command is disabled (as before),
/// else its name and shortcut when it has help to show below, else nothing (no tooltip).
pub fn row_tip(command: &str, label: &str, shortcut: Option<&str>, disabled_reason: Option<&str>) -> Option<String> {
    match disabled_reason {
        Some(why) => Some(why.to_owned()),
        None => help(command).map(|_| format!("{label}{}", shortcut.map(|k| format!("  ({k})")).unwrap_or_default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn catalog(source: &str) -> std::collections::BTreeMap<String, String> {
        serde_json::from_str(source).unwrap()
    }

    /// The `help.<command id>` messages; other `help.*` messages (About dialog texts) are not tool help.
    fn help_keys(source: &str) -> BTreeSet<String> {
        catalog(source)
            .into_keys()
            .filter(|k| k.starts_with("help.") && (k.starts_with("help.solid.") || DOCUMENTED.contains(&&k["help.".len()..])))
            .collect()
    }

    #[test]
    fn every_documented_id_is_a_real_command() {
        for id in DOCUMENTED {
            assert!(solidcraft_engine::find_command(id).is_some(), "{id} is not a command");
        }
        assert!(DOCUMENTED.is_sorted());
    }

    #[test]
    fn every_help_message_is_for_a_documented_command() {
        let english = help_keys(include_str!("../../../assets/locales/en.json"));
        let wanted: BTreeSet<String> = DOCUMENTED.iter().map(|id| format!("help.{id}")).collect();
        assert_eq!(english, wanted);
        for key in &english {
            let id = key.trim_start_matches("help.");
            assert!(solidcraft_engine::find_command(id).is_some(), "{key} names no command");
        }
    }

    #[test]
    fn the_help_texts_are_written_in_every_complete_language() {
        let english = catalog(include_str!("../../../assets/locales/en.json"));
        for source in [include_str!("../../../assets/locales/ja.json"), include_str!("../../../assets/locales/zh.json")] {
            let other = catalog(source);
            for (key, text) in english.iter().filter(|(k, _)| k.starts_with("help.")) {
                let translated = other.get(key).unwrap_or_else(|| panic!("{key} missing"));
                assert!(!translated.trim().is_empty());
                // The messages carry no placeholders; if one is added, both sides must agree.
                assert_eq!(text.matches('{').count(), translated.matches('{').count(), "{key}");
            }
        }
    }

    #[test]
    fn every_picture_is_for_a_documented_command_and_stays_in_its_band() {
        assert!(ILLUSTRATIONS.is_sorted_by_key(|(id, _)| *id));
        for (id, source) in ILLUSTRATIONS {
            assert!(DOCUMENTED.contains(id), "{id} has a picture but no help");
            let doc = icon_svg::Doc::parse(source).unwrap_or_else(|e| panic!("{id}: {e}"));
            for dark in [false, true] {
                let look = Look { dark, accent: None, vision: crate::theme::ColorVision::Off };
                let full = icon_svg::render(&doc, look, 128).unwrap_or_else(|| panic!("{id}: no image"));
                let rows = |range: std::ops::Range<usize>| {
                    full.data().get(range.start * 512..range.end * 512).unwrap_or_default().iter().skip(3).step_by(4).any(|a| *a > 0)
                };
                assert!(rows(16..112), "{id}: empty picture");
                assert!(!rows(0..16) && !rows(112..128), "{id}: the artwork leaves the 4:3 band");
                let image = band(source, look, 240).unwrap_or_else(|| panic!("{id}: no band"));
                assert_eq!(image.size, [240, 180]);
            }
        }
    }

    #[test]
    fn a_picture_follows_the_workspace_accent_and_keeps_its_shape() {
        let alpha = |image: &egui::ColorImage| image.pixels.iter().map(|c| c.a()).collect::<Vec<_>>();
        for (id, source) in ILLUSTRATIONS {
            for dark in [false, true] {
                let look = |accent| Look { dark, accent, vision: crate::theme::ColorVision::Off };
                let blue = band(source, look(None), 240).unwrap_or_else(|| panic!("{id}: no band"));
                for name in ["surface", "form", "mesh", "sketch"] {
                    let accent = icon_svg::accent_index(name).unwrap_or_else(|| panic!("accent {name}"));
                    let image = band(source, look(Some(accent)), 240).unwrap_or_else(|| panic!("{id}: no band"));
                    assert_ne!(image.pixels, blue.pixels, "{id}: the {name} accent changes the colours");
                    assert_eq!(alpha(&image), alpha(&blue), "{id}: the {name} accent keeps the shape");
                }
            }
        }
    }

    #[test]
    fn the_picture_takes_the_accent_of_the_active_tab() {
        let ctx = crate::fonts::test_context();
        let id = |ctx: &egui::Context| picture(ctx, "solid.loft").map(|t| t.id());
        let blue = id(&ctx);
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_tab_icon_accent"), Some("surface")));
        let orange = id(&ctx);
        assert!(blue.is_some() && orange.is_some() && blue != orange, "a tab accent gets its own texture");
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_tab_icon_accent"), None::<&'static str>));
        assert_eq!(id(&ctx), blue, "the base blue texture is reused");
    }

    #[test]
    fn a_picture_is_drawn_for_an_illustrated_command_only() {
        let ctx = crate::fonts::test_context();
        assert!(picture(&ctx, "solid.sweep").is_some_and(|t| t.size() == [240, 180]));
        assert!(picture(&ctx, "solid.extrude").is_none());
    }

    #[test]
    fn a_documented_command_has_help_and_others_do_not() {
        for id in DOCUMENTED {
            assert!(help(id).is_some_and(|text| !text.is_empty()), "{id}");
        }
        assert!(help("solid.draft").is_none());
    }

    #[test]
    fn a_drop_down_row_has_a_tip_only_for_a_reason_or_help() {
        assert_eq!(row_tip("solid.shell", "Shell", Some("S"), None).as_deref(), Some("Shell  (S)"));
        assert_eq!(row_tip("solid.hole", "Hole", None, None).as_deref(), Some("Hole"));
        assert_eq!(row_tip("solid.hole", "Hole", None, Some("Select a face")).as_deref(), Some("Select a face"));
        assert_eq!(row_tip("solid.draft", "Draft", None, None), None);
        assert_eq!(row_tip("solid.draft", "Draft", None, Some("No body")).as_deref(), Some("No body"));
    }
}
