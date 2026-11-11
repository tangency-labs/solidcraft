//! What installing SolidCraft's look gives the kit.
use super::*;
use solidcraft_theme::{AccentRamp, TabStyle, TabTable};
use solidcraft_ui_kit::{icons as kit_icons, theme_files, type_face};

/// The host's tab table, which is not the look's: it has none of the tabs the built-in themes
/// key per-tab surfaces by (Onyx's `tabs`), and every built-in loads under it.
const TABS: TabTable = TabTable::new(&[
    TabStyle { name: "DRAW", ramp: AccentRamp::Main, icon_accent: None },
    TabStyle { name: "SHAPE", ramp: AccentRamp::Surface, icon_accent: Some("surface") },
    TabStyle { name: "PAINT", ramp: AccentRamp::Render, icon_accent: None },
]);

#[test]
fn install_gives_the_kit_solidcrafts_icons_glyphs_fonts_and_themes() {
    solidcraft_theme::install_tabs(TABS);
    let ctx = egui::Context::default();
    install(&ctx);
    install(&ctx);
    // The icon set is SolidCraft's, the same static set, and draws its artwork.
    assert!(std::ptr::eq(kit_icons::installed(), &icons::SET));
    assert!(!kit_icons::resolve("extrude").placeholder);
    assert_eq!(kit_icons::installed().master_ids(), icons::SET.master_ids());
    // The glyphs are SolidCraft's: the compass ends have its pointer target.
    assert_eq!(kit_icons::glyphs::axis_radius(), compass::AXIS_RADIUS);
    // The built-in themes are every shipped file, each read.
    assert!(!themes::BUILTIN.is_empty());
    assert_eq!(theme_files::builtin().len(), themes::BUILTIN.len());
    // The fonts: Plex leads every weight family and the monospace one once a frame has begun.
    ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    let installed = ctx.fonts(|f| f.definitions().clone());
    let bundled = fonts::definitions();
    for family in type_face::sans_families().into_iter().chain([egui::FontFamily::Monospace]) {
        assert_eq!(installed.families.get(&family), bundled.families.get(&family), "{family:?}");
    }
    assert_eq!(installed.font_data["plex-sans"].bytes(), fonts::PLEX_SANS);
    assert_eq!(installed.font_data["plex-mono"].bytes(), fonts::PLEX_MONO);
    assert!(ctx.fonts_mut(|f| f.has_glyphs(&type_face::semibold(14.0), "Extrude")));
    let heading = ctx.global_style().text_styles[&egui::TextStyle::Heading].family.clone();
    assert_eq!(heading, type_face::semibold(1.0).family);
}

/// Under a host whose tab table has none of SolidCraft's tabs, every built-in theme loads, Onyx
/// without the per-tab surfaces it keys by SolidCraft's tab names.
#[test]
fn every_builtin_theme_loads_under_a_host_with_other_tabs() {
    solidcraft_theme::install_tabs(TABS);
    // The look's icon set, which the built-ins' icon palettes are read against.
    install_tables();
    let read = theme_files::read_builtin(themes::BUILTIN).unwrap_or_else(|e| panic!("the built-in themes read: {e}"));
    assert_eq!(read.len(), themes::BUILTIN.len());
    let onyx = read.iter().find(|theme| theme.id == "onyx").expect("Onyx is built in");
    assert!(onyx.tabs.is_empty(), "{:?}", onyx.tabs.keys());
}
