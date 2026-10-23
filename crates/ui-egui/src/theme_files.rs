//! Colour themes as files (#2255): a `solidcraft-theme/1` JSON file names a base palette (the
//! built-in Dark or Light) and sets any of its [`Tokens`] by name, so a theme can be a handful of
//! lines. Built-in themes (Gruvbox and others) are such files under `assets/styles/ui/themes/`;
//! imported ones are kept in the preferences (`Prefs::custom_themes`), so they also work in the
//! browser. Colour vision modes still apply on top of a theme (`Tokens::with_vision`).
//!
//! ```json
//! {"format": "solidcraft-theme/1", "name": "Mine", "base": "dark",
//!  "colors": {"panel": "#282828", "triad": ["#fb4934", "#b8bb26", "#83a598"]}}
//! ```

use std::collections::BTreeMap;
use std::sync::OnceLock;

use egui::Color32;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::theme::{IconColors, Tokens};

/// The `format` tag of a theme file.
pub const FORMAT: &str = "solidcraft-theme/1";
/// Hostile-input caps: file size, colours per file, name length, imported themes kept.
const MAX_BYTES: usize = 256 * 1024;
pub const MAX_COLORS: usize = 512;
const MAX_NAME: usize = 80;
pub const MAX_CUSTOM: usize = 64;

// Older exports contain colours for retired window bands and procedural icons.
// Discard them at import; they have no token, renderer, or exported setting now.
const RETIRED_COLORS: &[&str] = &[
    "icon_fill",
    "heading_bg",
    "subheading_bg",
    "app_bar_icon",
    "icons.sheet",
    "icons.sheet_2",
    "icons.sheet_light",
    "icons.sheet_line",
    "icons.glint",
    "icons.folder",
    "icons.plane",
    "icons.fold",
    "icons.stripe",
    "icons.sky",
    "icons.floor",
    "icons.paint",
    "icons.axes",
    "icons.access",
    "icons.draft",
    "icons.curvature",
    "icons.interference",
    "icons.warning",
];

/// The palette a theme starts from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Base {
    #[default]
    Dark,
    Light,
}

impl Base {
    pub fn is_dark(self) -> bool {
        self == Base::Dark
    }
}

/// A theme: its id (the `theme` preference's value), name, base and colour overrides, each a
/// `#rrggbb` / `#rrggbbaa` string or a list of them, keyed by [`keys`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThemeFile {
    pub id: String,
    pub name: String,
    pub base: Base,
    pub colors: BTreeMap<String, Value>,
}

/// The settable colours: a key, and where it lives in [`Tokens`]. `one` keys take a colour,
/// `many` keys a list of exactly as many colours as the token has; `icons.*` are the colours
/// viewport glyphs draw of their own. The destructuring below fails to build when a token is added
/// without a key here, so every colour stays themeable.
macro_rules! theme_keys {
    (one: [$($one:ident),* $(,)?], many: [$($many:ident),* $(,)?],
     icons: [$($ione:ident),* $(,)?] $(,)?) => {
        /// Every key a theme file may set, in the order an export writes them.
        pub fn keys() -> Vec<&'static str> {
            let mut k = vec![$(stringify!($one),)* $(stringify!($many),)*];
            k.extend([$(concat!("icons.", stringify!($ione)),)*]);
            k
        }

        fn slot<'a>(t: &'a mut Tokens, key: &str) -> Option<&'a mut [Color32]> {
            match key {
                $(stringify!($one) => Some(std::slice::from_mut(&mut t.$one)),)*
                $(stringify!($many) => Some(&mut t.$many[..]),)*
                $(concat!("icons.", stringify!($ione)) => Some(std::slice::from_mut(&mut t.icons.$ione)),)*
                _ => None,
            }
        }

        #[allow(dead_code)]
        fn every_colour_has_a_key(t: &Tokens) {
            // Not colours: the hover lift is a grey step, the analysis colours belong to shaders.
            let Tokens {
                $($one: _,)* $($many: _,)*
                hover_face_lift: _, analysis: _, icons: _,
                file_action_height: _, file_back_height: _, file_recovery_icon: _, file_grid_gap: _,
                file_nav_width: _, file_nav_height: _, file_tile_height: _, file_tile_icon: _, file_recent_height: _,
                file_sample_image_height: _, file_content_inset_x: _, file_content_inset_y: _, file_section_gap: _, font_start_title: _,
                space_2: _, space_3: _, space_4: _, space_6: _, space_8: _, space_10: _,
                space_12: _, space_14: _, space_16: _, space_24: _, radius_sm: _, radius_md: _,
                radius_lg: _, radius_xl: _, font_badge: _, font_micro: _, font_panel: _, font_small: _,
                font_caption: _, font_body: _, font_label: _, font_button: _, font_heading: _, font_triad: _,
                font_compass: _, font_subtitle: _, font_intro: _, font_article_heading: _, font_title: _, font_display: _,
                control_height: _,  control_radius: _, form_label_width: _, form_gap_y: _, form_footer_height: _, form_header_height: _,
                form_card_width: _, form_icon_size: _, form_close_size: _, form_inset: _, form_icon_inset: _, form_section_icon_size: _,
                form_checkbox_height: _, form_button_height: _, sketch_palette_width: _, sketch_mode_size: _, sketch_mode_radius: _, sketch_mode_border: _,
                sketch_palette_alpha: _, sketch_mode_tint: _, sketch_footer_icon_size: _, font_section_tracking: _,
            } = t;
            let IconColors { $($ione: _,)* } = &t.icons;
        }
    };
}

theme_keys! {
    one: [
        file_dim, file_recovery_well, file_background, file_surface, file_border, file_image_tint, file_rail, file_rail_ink, file_rail_hover,
        file_primary, file_primary_ink, file_primary_border, file_recovery, file_recovery_ink, file_recovery_border,
        app_bar, toolbar, panel, panel_header, border, text, text_dim, accent, accent_soft, sketch_accent, error, warning,
        icon, timeline, hover, sel_face, sel_edge, sel_edge_rim, hover_edge, hover_edge_halo, sel_profile,
        sel_profile_edge, hover_profile, hover_profile_edge, sel_vertex, origin_plane, origin_plane_edge, origin_plane_hover,
        origin_point, axis_x, axis_y, axis_z, construction_plane, construction_geom, body, body_edge, manipulator,
        preview_add, preview_new, preview_sketch, preview_cut, cube_face, cube_border, cube_text, cube_hover, viewport_top,
        viewport_bottom, grid, grid_major, body_selected, sketch, sketch_fixed, sketch_locked, dimension, dimension_driven,
        sketch_point, sketch_point_free, dialog_bg, dialog_border, chip, section_cap,
        profile_fill, glyph_bg, glyph_edge, glyph_text, overlay, rubber_band, app_bar_text, app_bar_hover, timeline_item,
        timeline_item_error, timeline_marker, field, box_window, box_crossing, sketch_projected, sketch_lost,
        sketch_construction, sketch_centerline, bend_up, bend_down, interference,
        sketch_ink, sketch_soft, surface_fill, surface_ink, surface_soft, mesh_fill, mesh_ink, mesh_soft, form_fill, form_tab_ink, form_soft, sheet_fill, sheet_ink, sheet_soft, render_fill, render_ink, render_soft, tab_on_fill_dark,
        app_bar_separator, surface_accent, mesh_accent, form_accent, render_accent, on_accent, agent_ok, agent_busy, app_bar_hover_subtle,
        app_bar_hover_button, app_bar_hover_strong, app_bar_tab_active,
        form_ink, form_dim, form_primary, form_error, form_error_fill, form_success, form_success_fill, dialog_body, dialog_footer, dialog_rule, control_fill, control_border, dialog_label, selected_ink, selected_bg,
    ],
    many: [triad, gizmo, continuity, logo],
    icons: [highlight, agent, marker_halo],
}

/// `#rrggbb` or `#rrggbbaa` (alpha not premultiplied).
fn parse_color(s: &str) -> Option<Color32> {
    let hex = s.trim().strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, a))
}

fn format_color(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// The colours a value gives for a slot of `len` colours, or why not.
fn colors_of(v: &Value, len: usize) -> Result<Vec<Color32>, String> {
    let list: Vec<&Value> = match v {
        Value::String(_) if len == 1 => vec![v],
        Value::Array(a) if a.len() == len => a.iter().collect(),
        _ if len == 1 => return Err("must be a colour such as \"#282828\"".into()),
        _ => return Err(format!("must be a list of {len} colours")),
    };
    list.into_iter().map(|c| c.as_str().and_then(parse_color).ok_or_else(|| format!("`{c}` is not a colour such as \"#282828\""))).collect()
}

/// A theme file's text, checked: the format tag, a name, a base and only known keys with valid
/// colours. Nothing is taken from a file with any error; the message names every bad key.
pub fn parse(text: &str) -> Result<ThemeFile, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("a theme file is at most {} KiB", MAX_BYTES / 1024));
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not a theme file: {e}"))?;
    from_value(&v)
}

/// [`parse`] for JSON already read (`ui.prefs` `theme_import`).
pub fn from_value(v: &Value) -> Result<ThemeFile, String> {
    if v.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(format!("not a theme file: `format` must be \"{FORMAT}\""));
    }
    let name = v.get("name").and_then(Value::as_str).map(str::trim).unwrap_or_default();
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err(format!("`name` must be 1 to {MAX_NAME} characters"));
    }
    let base = match v.get("base").and_then(Value::as_str) {
        Some("dark") => Base::Dark,
        Some("light") => Base::Light,
        _ => return Err("`base` must be \"dark\" or \"light\"".into()),
    };
    let mut src = match v.get("colors") {
        None => serde_json::Map::new(),
        Some(Value::Object(o)) => o.clone(),
        Some(_) => return Err("`colors` must be an object of colour names and values".into()),
    };
    if src.len() > MAX_COLORS {
        return Err(format!("`colors` has more than {MAX_COLORS} entries"));
    }
    src.retain(|key, _| !RETIRED_COLORS.contains(&key.as_str()));
    let mut probe = Tokens::light();
    let mut bad = Vec::new();
    for (k, c) in &src {
        match slot(&mut probe, k) {
            None => bad.push(format!("`{k}` is not a theme colour")),
            Some(s) => {
                if let Err(e) = colors_of(c, s.len()) {
                    bad.push(format!("`{k}` {e}"));
                }
            }
        }
    }
    if !bad.is_empty() {
        bad.truncate(8);
        return Err(bad.join("; "));
    }
    Ok(ThemeFile { id: slug(name), name: name.to_string(), base, colors: src.into_iter().collect() })
}

/// An id from a name: lower-case letters, digits and dashes.
fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() { "theme".into() } else { s }
}

/// The theme's palette: its base with its colours on top. A colour the saved preferences hold
/// that this version can't read (a hand edit) is skipped, never an error.
pub fn resolve(theme: &ThemeFile) -> Tokens {
    let mut t = if theme.base.is_dark() { Tokens::dark() } else { Tokens::light() };
    for (k, v) in &theme.colors {
        if let Some(s) = slot(&mut t, k)
            && let Ok(c) = colors_of(v, s.len())
        {
            s.copy_from_slice(&c);
        }
    }
    file_view_from_theme(&mut t, theme.base.is_dark(), |k| theme.colors.contains_key(k));
    t
}

/// The File view's colours a theme leaves unset follow the theme's own panel, border, accent and
/// text, not its base's fixed blue rail and grey cards (#4868). Each ink is whichever candidate
/// reads best on the fills it sits on.
fn file_view_from_theme(t: &mut Tokens, dark: bool, set: impl Fn(&str) -> bool) {
    use crate::theme::contrast_ratio;
    let fallback = t.text;
    let best = |inks: &[Color32], fills: &[Color32]| {
        let worst = |ink: Color32| fills.iter().map(|f| contrast_ratio(ink, *f)).fold(f64::INFINITY, f64::min);
        inks.iter().copied().max_by(|a, b| worst(*a).total_cmp(&worst(*b))).unwrap_or(fallback)
    };
    if !set("file_background") {
        t.file_background = t.panel;
    }
    if !set("file_surface") {
        t.file_surface = t.dialog_bg;
    }
    if !set("file_border") {
        t.file_border = t.border;
    }
    let inks = [t.on_accent, t.text, t.panel, t.app_bar, t.form_ink];
    let (rail_set, hover_set) = (set("file_rail"), set("file_rail_hover"));
    if !rail_set {
        // The accent deepened as the built-in palettes deepen theirs (Dark's rail is about 0.55 of
        // its accent, Light's about 0.85), toward the theme's own darkest colour.
        let (toward, k) = if dark { (t.app_bar, 0.45) } else { (t.text, 0.15) };
        t.file_rail = t.accent.lerp_to_gamma(toward, k);
    }
    if !hover_set {
        t.file_rail_hover = t.file_rail.lerp_to_gamma(t.accent, 0.25);
    }
    if !set("file_rail_ink") {
        t.file_rail_ink = best(&inks, &[t.file_rail, t.file_rail_hover]);
    }
    // A rail still too mid-toned for any ink goes further from the ink until the ink reads.
    let away = best(&inks, &[t.file_rail_ink]);
    for _ in 0..10 {
        let reads = |fill: Color32| contrast_ratio(t.file_rail_ink, fill) >= 4.5;
        if (rail_set || reads(t.file_rail)) && (hover_set || reads(t.file_rail_hover)) {
            break;
        }
        if !rail_set {
            t.file_rail = t.file_rail.lerp_to_gamma(away, 0.1);
        }
        if !hover_set {
            t.file_rail_hover = t.file_rail_hover.lerp_to_gamma(away, 0.1);
        }
    }
    if !set("file_primary") {
        t.file_primary = t.accent_soft;
    }
    if !set("file_primary_border") {
        t.file_primary_border = t.accent;
    }
    if !set("file_primary_ink") {
        t.file_primary_ink = best(&[t.accent, t.text, t.on_accent], &[t.file_primary, t.file_background]);
    }
    if !set("file_dim") {
        // Dim where dim text still reads on the page and the cards, else the plain text colour.
        let on = [t.file_surface, t.file_background];
        t.file_dim = if on.iter().all(|f| contrast_ratio(t.text_dim, *f) >= 4.5) { t.text_dim } else { best(&[t.text, t.form_ink], &on) };
    }
}

/// A complete theme file of a palette: every key, so it is a starting point for a new theme.
pub fn export(name: &str, base: Base, t: &Tokens) -> Value {
    let mut t = *t;
    let mut colors = serde_json::Map::new();
    for k in keys() {
        if let Some(s) = slot(&mut t, k) {
            let v = if let [c] = s { json!(format_color(*c)) } else { json!(s.iter().map(|c| format_color(*c)).collect::<Vec<_>>()) };
            colors.insert(k.to_string(), v);
        }
    }
    json!({"format": FORMAT, "name": name, "base": base, "colors": colors})
}

/// The theme files that come with SolidCraft (original files; Gruvbox, Tokyo Night, Solarized, Nord, Nosferatu and Catppuccin map open-source palettes, MIT).
const BUILTIN: [(&str, &str); 20] = [
    ("gruvbox-dark", include_str!("../../../assets/styles/ui/themes/gruvbox-dark.json")),
    ("gruvbox-dark-hard", include_str!("../../../assets/styles/ui/themes/gruvbox-dark-hard.json")),
    ("gruvbox-dark-soft", include_str!("../../../assets/styles/ui/themes/gruvbox-dark-soft.json")),
    ("gruvbox-light", include_str!("../../../assets/styles/ui/themes/gruvbox-light.json")),
    ("gruvbox-light-hard", include_str!("../../../assets/styles/ui/themes/gruvbox-light-hard.json")),
    ("gruvbox-light-soft", include_str!("../../../assets/styles/ui/themes/gruvbox-light-soft.json")),
    ("solidcraft-95", include_str!("../../../assets/styles/ui/themes/solidcraft-95.json")),
    ("platinum-9", include_str!("../../../assets/styles/ui/themes/platinum-9.json")),
    ("cupertino", include_str!("../../../assets/styles/ui/themes/cupertino.json")),
    ("green-phosphor", include_str!("../../../assets/styles/ui/themes/green-phosphor.json")),
    ("amber-phosphor", include_str!("../../../assets/styles/ui/themes/amber-phosphor.json")),
    ("blueprint", include_str!("../../../assets/styles/ui/themes/blueprint.json")),
    ("one-dark", include_str!("../../../assets/styles/ui/themes/one-dark.json")),
    ("tokyo-night", include_str!("../../../assets/styles/ui/themes/tokyo-night.json")),
    ("solarized-light", include_str!("../../../assets/styles/ui/themes/solarized-light.json")),
    ("nord", include_str!("../../../assets/styles/ui/themes/nord.json")),
    ("high-contrast", include_str!("../../../assets/styles/ui/themes/high-contrast.json")),
    ("graphite", include_str!("../../../assets/styles/ui/themes/graphite.json")),
    ("nosferatu", include_str!("../../../assets/styles/ui/themes/nosferatu.json")),
    ("catppuccin-mocha", include_str!("../../../assets/styles/ui/themes/catppuccin-mocha.json")),
];

/// The built-in themes, read once (one that fails to read is left out; a test reads them all).
pub fn builtin() -> &'static [ThemeFile] {
    static THEMES: OnceLock<Vec<ThemeFile>> = OnceLock::new();
    THEMES.get_or_init(|| {
        BUILTIN
            .iter()
            .filter_map(|(id, text)| match parse(text) {
                Ok(t) => Some(ThemeFile { id: (*id).to_string(), ..t }),
                Err(e) => {
                    log::error!("built-in theme {id}: {e}");
                    None
                }
            })
            .collect()
    })
}

/// The theme a `theme` preference names: built in, or imported. `None` for dark, light, system
/// or an unknown id.
pub fn find<'a>(id: &str, custom: &'a [ThemeFile]) -> Option<&'a ThemeFile> {
    builtin().iter().find(|t| t.id == id).or_else(|| custom.iter().find(|t| t.id == id))
}

/// Add an imported theme to `custom`, replacing one with the same id (importing a file again
/// updates it). Its id never takes a built-in one's or the modes' (dark, light, system).
/// Returns the id it is kept under.
pub fn add(custom: &mut Vec<ThemeFile>, mut theme: ThemeFile) -> Result<String, String> {
    if ["dark", "light", "system"].contains(&theme.id.as_str()) || builtin().iter().any(|t| t.id == theme.id) {
        theme.id = format!("custom-{}", theme.id);
    }
    let id = theme.id.clone();
    match custom.iter().position(|t| t.id == id) {
        Some(i) => {
            if let Some(old) = custom.get_mut(i) {
                *old = theme;
            }
        }
        None if custom.len() >= MAX_CUSTOM => return Err(format!("at most {MAX_CUSTOM} imported themes; remove one first")),
        None => custom.push(theme),
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_read_and_differ_from_their_base() {
        assert_eq!(builtin().len(), BUILTIN.len(), "every built-in theme file reads");
        for t in builtin() {
            let base = if t.base.is_dark() { Tokens::dark() } else { Tokens::light() };
            assert_ne!(resolve(t), base, "{}", t.id);
            if t.id.starts_with("gruvbox-") {
                assert_eq!(t.base.is_dark(), t.id.contains("dark"), "{}", t.id);
            }
        }
    }

    /// #4868: a theme that sets none of the File view's colours (all the built-in ones) gets them
    /// from its own palette, readable: the rail and primary action from its accent, the page from
    /// its panel, not Dark's or Light's blue and grey. One it does set stays as set.
    #[test]
    fn the_file_view_follows_a_theme_that_does_not_set_its_colours() {
        for theme in builtin() {
            let t = resolve(theme);
            if !theme.colors.contains_key("file_rail") {
                assert_eq!((t.file_background, t.file_primary), (t.panel, t.accent_soft), "{}", theme.id);
                let base = if theme.base.is_dark() { Tokens::dark() } else { Tokens::light() };
                assert!(t.accent == base.accent || t.file_rail != base.file_rail, "{}: the rail is the base's", theme.id);
            }
            // 4.5:1, or the best any of the theme's own inks reaches on that fill when that is less
            // (Solarized Light's text on its soft accent is 4.29:1): a derived colour can't do
            // better than the theme itself.
            let own = [t.text, t.form_ink, t.accent, t.on_accent];
            let floor = |fill: Color32| own.iter().map(|i| crate::theme::contrast_ratio(*i, fill)).fold(0.0, f64::max).min(4.5) - 1e-6;
            for (ink, fill) in [
                (t.file_rail_ink, t.file_rail),
                (t.file_rail_ink, t.file_rail_hover),
                (t.file_primary_ink, t.file_primary),
                (t.file_primary_ink, t.file_background),
                (t.file_dim, t.file_surface),
                (t.file_dim, t.file_background),
            ] {
                let ratio = crate::theme::contrast_ratio(ink, fill);
                assert!(ratio >= floor(fill), "{}: {ink:?} on {fill:?} is {ratio:.2}:1 (floor {:.2})", theme.id, floor(fill));
            }
        }
        let green = builtin().iter().find(|t| t.id == "green-phosphor").unwrap();
        assert_ne!(resolve(green).file_rail, Tokens::dark().file_rail, "Green Phosphor's rail isn't Dark's blue");
        let own = parse(r##"{"format":"solidcraft-theme/1","name":"Own rail","base":"dark","colors":{"accent":"#ff0000","file_rail":"#102030"}}"##)
            .unwrap();
        assert_eq!(resolve(&own).file_rail, Color32::from_rgb(0x10, 0x20, 0x30));
    }

    #[test]
    fn a_short_file_sets_only_its_colours() {
        let t = parse(r##"{"format":"solidcraft-theme/1","name":"My Theme!","base":"light","colors":{"panel":"#102030","triad":["#ff0000","#00ff00","#0000ff80"]}}"##).unwrap();
        assert_eq!((t.id.as_str(), t.name.as_str(), t.base), ("my-theme", "My Theme!", Base::Light));
        let p = resolve(&t);
        assert_eq!(p.panel, Color32::from_rgb(0x10, 0x20, 0x30));
        assert_eq!(p.triad[2], Color32::from_rgba_unmultiplied(0, 0, 255, 0x80));
        assert_eq!(p.text, Tokens::light().text);
    }

    #[test]
    fn older_exports_keep_active_overrides_without_retired_ui_settings() {
        let mut file = export("Older theme", Base::Dark, &Tokens::dark());
        let colors = file.get_mut("colors").and_then(Value::as_object_mut).unwrap();
        colors.insert("panel".into(), json!("#102030"));
        for key in RETIRED_COLORS {
            colors.insert((*key).into(), json!("#abcdef"));
        }
        let imported = from_value(&file).unwrap();
        assert_eq!(resolve(&imported).panel, Color32::from_rgb(0x10, 0x20, 0x30));
        assert_eq!(imported.colors.len(), keys().len());
        assert!(RETIRED_COLORS.iter().all(|key| !imported.colors.contains_key(*key)));
    }

    #[test]
    fn bad_files_are_refused_naming_every_bad_key() {
        let ok = r#""format":"solidcraft-theme/1","name":"x","base":"dark""#;
        for (text, want) in [
            ("[]".to_string(), "format"),
            (r#"{"format":"solidcraft-theme/2","name":"x","base":"dark"}"#.into(), "format"),
            (r#"{"format":"solidcraft-theme/1","name":" ","base":"dark"}"#.into(), "name"),
            (r#"{"format":"solidcraft-theme/1","name":"x","base":"blue"}"#.into(), "base"),
            (format!(r#"{{{ok},"colors":[]}}"#), "colors"),
            (format!(r##"{{{ok},"colors":{{"panle":"#000000","text":"red","triad":["#000000"],"icons.axes":"#000000"}}}}"##), "panle"),
            ("x".repeat(MAX_BYTES + 1), "KiB"),
        ] {
            let e = parse(&text).unwrap_err();
            assert!(e.contains(want), "{text:.80}: {e}");
        }
        let e = parse(&format!(r##"{{{ok},"colors":{{"panle":"#000000","text":"red","triad":["#000000"]}}}}"##)).unwrap_err();
        for k in ["panle", "text", "triad"] {
            assert!(e.contains(k), "{e}");
        }
        for c in ["#12345", "#1234567", "#gggggg", "282828", "#2828281"] {
            assert!(parse_color(c).is_none(), "{c}");
        }
    }

    /// An export is a complete theme: importing it gives the same palette (to a unit of rounding
    /// for translucent colours), on either base.
    #[test]
    fn export_then_import_gives_the_palette_back() {
        for (base, palette) in [(Base::Dark, Tokens::dark()), (Base::Light, Tokens::light())] {
            let file = export("Round trip", base, &palette);
            let t = from_value(&file).unwrap();
            assert_eq!(t.colors.len(), keys().len());
            let other = ThemeFile { base: if base.is_dark() { Base::Light } else { Base::Dark }, ..t.clone() };
            for back in [resolve(&t), resolve(&other)] {
                let (mut a, mut b) = (palette, back);
                for k in keys() {
                    let (Some(x), Some(y)) = (slot(&mut a, k).map(|s| s.to_vec()), slot(&mut b, k).map(|s| s.to_vec())) else { continue };
                    for (x, y) in x.iter().zip(&y) {
                        let close = x.to_array().iter().zip(y.to_array()).all(|(p, q)| p.abs_diff(q) <= 1);
                        assert!(close, "{k}: {x:?} vs {y:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn imported_ids_never_take_a_built_in_or_mode_id_and_replace_their_own() {
        let mut custom = Vec::new();
        let mk = |name: &str| ThemeFile { id: slug(name), name: name.into(), ..Default::default() };
        assert_eq!(add(&mut custom, mk("Gruvbox Dark")).unwrap(), "custom-gruvbox-dark");
        assert_eq!(add(&mut custom, mk("Dark")).unwrap(), "custom-dark");
        assert_eq!(add(&mut custom, mk("Mine")).unwrap(), "mine");
        let again = ThemeFile { base: Base::Light, ..mk("Mine") };
        assert_eq!(add(&mut custom, again).unwrap(), "mine");
        assert_eq!(custom.len(), 3);
        assert_eq!(custom.iter().find(|t| t.id == "mine").map(|t| t.base), Some(Base::Light));
        for i in custom.len()..MAX_CUSTOM {
            add(&mut custom, mk(&format!("t{i}"))).unwrap();
        }
        assert!(add(&mut custom, mk("one too many")).is_err());
        assert!(find("gruvbox-dark", &custom).is_some() && find("mine", &custom).is_some() && find("dark", &custom).is_none());
    }
}
