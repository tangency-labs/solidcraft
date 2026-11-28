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

/// Text and icons read on the surfaces the UI paints them on, in Dark, Light and every built-in
/// theme (#9468): 4.5:1 for body text, 3:1 for dim labels, icons, bold labels and marks. Inks the
/// kit chooses for a fill go through `ink_on`; the rest are the theme's own pairs.
#[test]
fn every_builtin_theme_keeps_text_and_icons_readable_on_their_surfaces() {
    install_tables();
    let read = theme_files::read_builtin(themes::BUILTIN).unwrap_or_else(|e| panic!("the built-in themes read: {e}"));
    let mut themes = vec![("dark".to_string(), solidcraft_theme::Tokens::dark()), ("light".to_string(), solidcraft_theme::Tokens::light())];
    themes.extend(read.iter().map(|t| (t.id.clone(), theme_files::resolve(t))));
    let c = solidcraft_theme::contrast_ratio;
    let over = solidcraft_theme::composite;
    let ink_on = solidcraft_ui_kit::ink_on;
    let mut failures = Vec::new();
    for (id, t) in &themes {
        let selected = over(t.selected_bg, t.dialog_body);
        let pairs = [
            ("text on panel", t.text, t.panel, 4.5),
            ("form_ink on dialog_body", t.form_ink, t.dialog_body, 4.5),
            ("text_dim on panel", t.text_dim, t.panel, 3.0),
            ("text_dim on toolbar", t.text_dim, t.toolbar, 3.0),
            ("text_dim on hover", t.text_dim, over(t.hover, t.panel), 3.0),
            ("form_dim on hover", t.form_dim, over(t.hover, t.dialog_body), 3.0),
            ("form_success on dialog_body", t.form_success, t.dialog_body, 3.0),
            ("selected_ink on selected_bg", t.selected_ink, selected, 4.5),
            ("navigation menu: selected row", ink_on(t, selected, t.form_primary, 3.0), selected, 3.0),
            ("navigation menu: selected row's detail", ink_on(t, selected, t.form_dim, 3.0), selected, 3.0),
            ("suggestions: picked row", ink_on(t, selected, t.form_primary, 4.5), selected, 4.5),
            ("footer toggle that is on", ink_on(t, selected, t.form_ink, 4.5), selected, 4.5),
        ];
        for (what, ink, fill, floor) in pairs {
            let ratio = c(ink, fill);
            if ratio < floor {
                failures.push(format!("{id}: {what} {ratio:.2}:1 (floor {floor})"));
            }
        }
    }
    assert!(failures.is_empty(), "below the floor:\n{}", failures.join("\n"));
}

/// The app bar's edit palette keeps its three levels apart and its hovered text readable on Onyx,
/// which sets the app bar's own colours over Dark. The kit checks its Dark and Light the same way
/// (`app_bar_edit_palette_has_three_distinct_levels_and_readable_foreground`), and the theme files
/// are the look's, so this half of the check lives here.
#[test]
fn app_bar_edit_palette_reads_on_onyx() {
    install_tables();
    let read = theme_files::read_builtin(themes::BUILTIN).unwrap_or_else(|e| panic!("the built-in themes read: {e}"));
    let onyx_file = read.iter().find(|theme| theme.id == "onyx").expect("Onyx is built in");
    let color = |name: &str| {
        onyx_file.colors.get(name).and_then(|v| v.as_str()).and_then(theme_files::parse_color).unwrap_or_else(|| panic!("Onyx's {name}"))
    };
    let t = solidcraft_theme::Tokens { app_bar: color("app_bar"), app_bar_text: color("app_bar_text"), ..solidcraft_theme::Tokens::dark() };
    let c = solidcraft_theme::contrast_ratio;
    let p = solidcraft_ui_kit::app_bar::edit_palette(t);
    let (a, b, foreground) = (c(p.card, p.item), c(p.item, p.hover), c(t.app_bar_text, p.hover));
    assert!(a >= 1.25 && b >= 1.25, "Onyx: separate levels {a}/{b}");
    assert!(foreground >= 4.5, "Onyx: hovered text remains legible {foreground}");
}
