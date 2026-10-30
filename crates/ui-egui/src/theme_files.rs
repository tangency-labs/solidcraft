//! Colour and shape themes as files (#2255): a `solidcraft-theme/1` JSON file names a base palette (the
//! built-in Dark or Light) and sets any of its [`Tokens`] by name, so a theme can be a handful of
//! lines. Built-in themes (Gruvbox and others) are such files under `assets/styles/ui/themes/`;
//! imported ones are kept in the preferences (`Prefs::custom_themes`), so they also work in the
//! browser. Colour vision modes still apply on top of a theme (`Tokens::with_vision`).
//!
//! ```json
//! {"format": "solidcraft-theme/1", "name": "Mine", "base": "dark",
//!  "colors": {"panel": "#282828", "axis_x": "#fb4934", "axis_y": "#b8bb26", "axis_z": "#83a598"}}
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

/// A theme: its id (the `theme` preference's value), name, base, optional radii and colour overrides, each a
/// `#rrggbb` / `#rrggbbaa` string or a list of them, keyed by [`keys`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThemeFile {
    pub id: String,
    pub name: String,
    pub base: Base,
    pub colors: BTreeMap<String, Value>,
    /// Optional corner radii in logical UI points. Missing entries keep the base values.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub radii: BTreeMap<String, u8>,
    /// Optional border roles; absent entries retain the existing surface-specific styling.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub borders: BTreeMap<String, Value>,
    /// SVG display roles and three-colour semantic ramps; omitted values keep compiled defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icon_palette: BTreeMap<String, Value>,
}

/// Maximum imported radius, in logical UI points; widgets clamp to their own bounds.
const MAX_RADIUS: u8 = 32;
const RADIUS_KEYS: [&str; 6] = ["radius_sm", "radius_md", "radius_lg", "radius_xl", "control_radius", "sketch_mode_radius"];

fn radius_slot<'a>(t: &'a mut Tokens, key: &str) -> Option<&'a mut u8> {
    match key {
        "radius_sm" => Some(&mut t.radius_sm),
        "radius_md" => Some(&mut t.radius_md),
        "radius_lg" => Some(&mut t.radius_lg),
        "radius_xl" => Some(&mut t.radius_xl),
        "control_radius" => Some(&mut t.control_radius),
        "sketch_mode_radius" => Some(&mut t.sketch_mode_radius),
        _ => None,
    }
}

impl ThemeFile {
    /// Validate direct control-channel preference updates as strictly as file imports.
    pub(crate) fn validate_radii(&self) -> Result<(), String> {
        for (key, value) in &self.radii {
            if !RADIUS_KEYS.contains(&key.as_str()) {
                return Err(format!("`radii.{key}` is not a theme radius"));
            }
            if *value > MAX_RADIUS {
                return Err(format!("`radii.{key}` must be an integer from 0 to {MAX_RADIUS}"));
            }
        }
        Ok(())
    }
}

/// Strictly bounded strokes shared by file imports and direct preference updates.
pub(crate) fn parse_borders(values: &BTreeMap<String, Value>) -> Result<crate::theme::Borders, String> {
    use crate::theme::{Bevel, Borders, InsetBorder};
    if values.len() > 7 {
        return Err("`borders` has more than seven roles".into());
    }
    let mut borders = Borders::default();
    for (key, value) in values {
        let obj = value.as_object().ok_or_else(|| format!("`borders.{key}` must be an object"))?;
        if key == "bevel" {
            if obj.keys().any(|k| !matches!(k.as_str(), "width" | "highlight" | "light" | "shadow" | "dark_shadow")) {
                return Err("`borders.bevel` has an unknown setting".into());
            }
            let width = obj
                .get("width")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v > 0.0 && *v <= 2.0)
                .ok_or("`borders.bevel.width` must be a finite number greater than 0 and at most 2")? as f32;
            let color = |name: &str| {
                obj.get(name)
                    .and_then(Value::as_str)
                    .and_then(parse_color)
                    .ok_or_else(|| format!("`borders.bevel.{name}` must be #rrggbb or #rrggbbaa"))
            };
            borders.bevel = Some(Bevel {
                width,
                highlight: color("highlight")?,
                light: color("light")?,
                shadow: color("shadow")?,
                dark_shadow: color("dark_shadow")?,
            });
            continue;
        }
        if obj.keys().any(|k| k != "color" && k != "width" && !(key == "primary_inset" && k == "gap") && !(key == "secondary" && k == "pattern")) {
            return Err(format!("`borders.{key}` has an unknown setting"));
        }
        let number = |name: &str, max: f64| -> Result<f32, String> {
            obj.get(name)
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v >= 0.0 && *v <= max)
                .map(|v| v as f32)
                .ok_or_else(|| format!("`borders.{key}.{name}` must be a finite number from 0 to {max}"))
        };
        let width = number("width", 4.0)?;
        let color = obj
            .get("color")
            .and_then(Value::as_str)
            .and_then(parse_color)
            .ok_or_else(|| format!("`borders.{key}.color` must be #rrggbb or #rrggbbaa"))?;
        let stroke = egui::Stroke::new(width, color);
        match key.as_str() {
            "primary" => borders.primary = Some(stroke),
            "secondary" => {
                borders.secondary = Some(stroke);
                borders.secondary_dotted = match obj.get("pattern") {
                    None => false,
                    Some(Value::String(pattern)) if pattern == "solid" => false,
                    Some(Value::String(pattern)) if pattern == "dotted" => true,
                    _ => return Err("`borders.secondary.pattern` must be solid or dotted".into()),
                };
            }
            "tertiary" => borders.tertiary = Some(stroke),
            "container" => borders.container = Some(stroke),
            "control" => borders.control = Some(stroke),
            "primary_inset" => borders.primary_inset = Some(InsetBorder { stroke, gap: number("gap", 12.0)? }),
            _ => return Err(format!("`borders.{key}` is not a border role")),
        }
    }
    Ok(borders)
}

/// Import only known, opaque SVG roles and bounded semantic ramps. GAP remains an eraser.
pub(crate) fn parse_icon_palette(values: &BTreeMap<String, Value>) -> Result<Option<crate::icon_svg::Palette>, String> {
    use crate::icon_svg::{ACCENT_NAMES, Palette, ROLE_NAMES};
    if values.is_empty() {
        return Ok(None);
    }
    if values.len() > ROLE_NAMES.len() + ACCENT_NAMES.len() + 1 {
        return Err("too many icon palette entries".into());
    }
    let mut palette = Palette::default();
    for (name, value) in values {
        let ramp = if name == "solid" { Some(0) } else { ACCENT_NAMES.iter().position(|a| *a == name).map(|a| a + 1) };
        let role = ROLE_NAMES.iter().position(|r| *r == name && *r != "gap");
        if ramp.is_none() && role.is_none() {
            return Err(format!("`icon_palette.{name}` is not an SVG role or semantic ramp"));
        }
        let colors = colors_of(value, if ramp.is_some() { 3 } else { 1 }).map_err(|e| format!("`icon_palette.{name}` {e}"))?;
        if colors.iter().any(|c| c.a() != 255) {
            return Err(format!("`icon_palette.{name}` must use opaque colours"));
        }
        let rgb = |c: &Color32| [c.r(), c.g(), c.b()];
        if let Some(slot) = ramp.and_then(|r| palette.accents.get_mut(r)) {
            if let [a, b, c] = colors.as_slice() {
                *slot = Some([rgb(a), rgb(b), rgb(c)]);
            }
        } else if let Some((slot, color)) = role.and_then(|r| palette.roles.get_mut(r)).zip(colors.first()) {
            *slot = Some(rgb(color));
        }
    }
    Ok(Some(palette))
}

fn export_icon_palette(palette: Option<crate::icon_svg::Palette>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    let Some(palette) = palette else { return out };
    let hex = |[r, g, b]: [u8; 3]| format!("#{r:02x}{g:02x}{b:02x}");
    for (name, color) in crate::icon_svg::ROLE_NAMES.iter().zip(palette.roles) {
        if let Some(rgb) = color {
            out.insert((*name).into(), json!(hex(rgb)));
        }
    }
    for (name, ramp) in std::iter::once("solid").chain(crate::icon_svg::ACCENT_NAMES.iter().copied()).zip(palette.accents) {
        if let Some(colors) = ramp {
            out.insert(name.into(), json!(colors.map(hex)));
        }
    }
    out
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

        /// Stable identity of resolved colours, including custom themes with the same base/id.
        /// Share the token inventory with import/export so added colour tokens cannot be missed.
        pub(crate) fn color_key(t: &Tokens) -> u64 {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            $(t.$one.hash(&mut h);)*
            $(t.$many.hash(&mut h);)*
            $(t.icons.$ione.hash(&mut h);)*
            h.finish()
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
                hover_face_lift: _, analysis: _, icons: _, borders: _, icon_palette: _,
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
        viewport_bottom, grid, grid_major, body_selected, sketch, sketch_fixed, sketch_halo, sketch_locked, dimension, dimension_driven,
        sketch_point, sketch_point_free, dialog_bg, dialog_border, chip, section_cap,
        profile_fill, glyph_bg, glyph_edge, glyph_text, overlay, rubber_band, app_bar_text, app_bar_hover, timeline_item,
        timeline_item_error, timeline_marker, field, box_window, box_crossing, sketch_projected, sketch_lost,
        sketch_construction, sketch_centerline, bend_up, bend_down, interference, comb_envelope, comb_teeth,
        sketch_ink, sketch_soft, surface_fill, surface_ink, surface_soft, mesh_fill, mesh_ink, mesh_soft, form_fill, form_tab_ink, form_soft, sheet_fill, sheet_ink, sheet_soft, render_fill, render_ink, render_soft, tab_on_fill_dark,
        app_bar_separator, surface_accent, mesh_accent, form_accent, render_accent, on_accent, agent_ok, agent_busy, app_bar_hover_subtle,
        app_bar_hover_button, app_bar_hover_strong, app_bar_tab_active,
        form_ink, form_dim, form_primary, form_error, form_error_fill, form_success, form_success_fill, dialog_body, dialog_footer, dialog_rule, control_fill, control_border, dialog_label, selected_ink, selected_bg,
    ],
    many: [continuity, logo, section_hatch],
    icons: [highlight, agent, marker_halo],
}

/// Keys of theme files from before every axis took one set of colours (#976): the view triad's
/// and the Move gizmo's X, Y and Z. Each still reads, as a list of three, and sets `axis_x`,
/// `axis_y` and `axis_z`, `triad` over `gizmo` and a file's own `axis_*` keys over both. An
/// export never writes them.
const LEGACY_AXES: [&str; 2] = ["gizmo", "triad"];

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

// The application mark now has three faces and no outline. Older theme exports and
// saved preferences carry a fourth, retired outline colour; keep their face colours.
fn colors_for_key(key: &str, value: &Value, len: usize) -> Result<Vec<Color32>, String> {
    if key == "logo"
        && len == 3
        && let Value::Array(colors) = value
        && colors.len() == 4
    {
        return colors_of(&Value::Array(colors.iter().take(len).cloned().collect()), len);
    }
    colors_of(value, len)
}

/// A theme file's text, checked: the format tag, a name, a base and only known keys with valid
/// colours and bounded radii. Nothing is taken from a file with any error; the message names every bad key.
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
        if LEGACY_AXES.contains(&k.as_str()) {
            if let Err(e) = colors_of(c, 3) {
                bad.push(format!("`{k}` {e}"));
            }
            continue;
        }
        match slot(&mut probe, k) {
            None => bad.push(format!("`{k}` is not a theme colour")),
            Some(s) => {
                if let Err(e) = colors_for_key(k, c, s.len()) {
                    bad.push(format!("`{k}` {e}"));
                }
            }
        }
    }
    let radii = match v.get("radii") {
        None => BTreeMap::new(),
        Some(Value::Object(values)) => {
            if values.len() > RADIUS_KEYS.len() {
                return Err(format!("`radii` has more than {} entries", RADIUS_KEYS.len()));
            }
            let mut radii = BTreeMap::new();
            for (key, value) in values {
                if !RADIUS_KEYS.contains(&key.as_str()) {
                    bad.push(format!("`radii.{key}` is not a theme radius"));
                } else if let Some(n) = value.as_u64().filter(|n| *n <= u64::from(MAX_RADIUS)) {
                    radii.insert(key.clone(), n as u8);
                } else {
                    bad.push(format!("`radii.{key}` must be an integer from 0 to {MAX_RADIUS}"));
                }
            }
            radii
        }
        Some(_) => return Err("`radii` must be an object of radius names and values".into()),
    };
    if !bad.is_empty() {
        bad.truncate(8);
        return Err(bad.join("; "));
    }
    let borders = match v.get("borders") {
        None => BTreeMap::new(),
        Some(Value::Object(values)) => values.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(_) => return Err("`borders` must be an object of border roles".into()),
    };
    parse_borders(&borders)?;
    let icon_palette = match v.get("icon_palette") {
        None => BTreeMap::new(),
        Some(Value::Object(values)) => values.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(_) => return Err("`icon_palette` must be an object of SVG roles and ramps".into()),
    };
    parse_icon_palette(&icon_palette)?;
    Ok(ThemeFile { id: slug(name), name: name.to_string(), base, colors: src.into_iter().collect(), radii, borders, icon_palette })
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

/// The theme's tokens: its base with its colours and radii on top. A colour the saved preferences hold
/// that this version can't read (a hand edit) is skipped, never an error.
pub fn resolve(theme: &ThemeFile) -> Tokens {
    let mut t = if theme.base.is_dark() { Tokens::dark() } else { Tokens::light() };
    for k in LEGACY_AXES {
        if let Some(Ok(c)) = theme.colors.get(k).map(|v| colors_of(v, 3))
            && let [x, y, z] = c[..]
        {
            (t.axis_x, t.axis_y, t.axis_z) = (x, y, z);
        }
    }
    for (k, v) in &theme.colors {
        if let Some(s) = slot(&mut t, k)
            && let Ok(c) = colors_for_key(k, v, s.len())
        {
            s.copy_from_slice(&c);
        }
    }
    for (key, value) in &theme.radii {
        if *value <= MAX_RADIUS
            && let Some(slot) = radius_slot(&mut t, key)
        {
            *slot = *value;
        }
    }
    // Malformed hand-edited saved settings cannot introduce non-finite drawing geometry.
    t.borders = parse_borders(&theme.borders).unwrap_or_default();
    t.icon_palette = parse_icon_palette(&theme.icon_palette).unwrap_or_default();
    // A file that sets no colours is its base palette, exactly.
    if !theme.colors.is_empty() {
        file_view_from_theme(&mut t, theme.base.is_dark(), |k| theme.colors.contains_key(k));
    }
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
    let radii: BTreeMap<_, _> = RADIUS_KEYS.into_iter().filter_map(|key| radius_slot(&mut t, key).map(|value| (key, *value))).collect();
    let mut borders = serde_json::Map::new();
    for (key, stroke) in [
        ("primary", t.borders.primary),
        ("secondary", t.borders.secondary),
        ("tertiary", t.borders.tertiary),
        ("container", t.borders.container),
        ("control", t.borders.control),
    ] {
        if let Some(stroke) = stroke {
            borders.insert(key.into(), json!({"width": stroke.width, "color": format_color(stroke.color)}));
        }
    }
    if t.borders.secondary_dotted
        && let Some(secondary) = borders.get_mut("secondary").and_then(Value::as_object_mut)
    {
        secondary.insert("pattern".into(), json!("dotted"));
    }
    if let Some(inset) = t.borders.primary_inset {
        borders.insert("primary_inset".into(), json!({"width": inset.stroke.width, "color": format_color(inset.stroke.color), "gap": inset.gap}));
    }
    if let Some(bevel) = t.borders.bevel {
        let shadow = format_color(bevel.shadow);
        borders.insert(
            "bevel".into(),
            json!({"width": bevel.width, "highlight": format_color(bevel.highlight),
            "light": format_color(bevel.light), "shadow": shadow, "dark_shadow": format_color(bevel.dark_shadow)}),
        );
    }
    let icon_palette = export_icon_palette(t.icon_palette);
    json!({"format": FORMAT, "name": name, "base": base, "colors": colors, "radii": radii, "borders": borders, "icon_palette": icon_palette})
}

/// The theme files that come with SolidCraft (original files; Gruvbox, Tokyo Night, Solarized, Nord, Nosferatu and Catppuccin map open-source palettes, MIT; Okibi and Akari map the Okibi design system; Clay and Clay Dark are original warm paper and charcoal palettes).
const BUILTIN: [(&str, &str); 24] = [
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
    ("okibi", include_str!("../../../assets/styles/ui/themes/okibi.json")),
    ("akari", include_str!("../../../assets/styles/ui/themes/akari.json")),
    ("clay", include_str!("../../../assets/styles/ui/themes/clay.json")),
    ("clay-dark", include_str!("../../../assets/styles/ui/themes/clay-dark.json")),
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
    fn border_roles_roundtrip_on_either_base_and_preserve_legacy_defaults() {
        use crate::theme::{BorderRole, Borders};
        let settings = json!({
            "primary":{"width":4,"color":"#ffffff"}, "secondary":{"width":1.5,"color":"#aabbcc","pattern":"dotted"},
            "container":{"width":1,"color":"#a8c8ed"},
            "tertiary":{"width":0,"color":"#00000000"}, "control":{"width":0.5,"color":"#123456"},
            "primary_inset":{"width":0.75,"color":"#ffffff80","gap":12}
        });
        for base in ["dark", "light"] {
            let old = from_value(&json!({"format":FORMAT,"name":"Old","base":base})).unwrap();
            assert_eq!(resolve(&old).borders, Borders::LEGACY);
            let legacy = egui::Stroke::new(2.5, Color32::RED);
            for role in [BorderRole::Primary, BorderRole::Secondary, BorderRole::Tertiary, BorderRole::Container, BorderRole::Control] {
                assert_eq!(resolve(&old).borders.stroke(role, legacy), legacy);
            }
            let file = from_value(&json!({"format":FORMAT,"name":"New","base":base,"borders":settings})).unwrap();
            let t = resolve(&file);
            assert_eq!(t.primary_border(Color32::BLACK).width, 4.0);
            assert!(t.borders.secondary_dotted);
            assert_eq!(t.tertiary_border(Color32::WHITE).width, 0.0);
            assert_eq!(t.control_border_stroke(Color32::WHITE).color, Color32::from_rgb(0x12, 0x34, 0x56));
            assert_eq!(resolve(&from_value(&export("New", file.base, &t)).unwrap()), t);
            assert_eq!(serde_json::from_value::<ThemeFile>(serde_json::to_value(&file).unwrap()).unwrap(), file);
        }
        let saved: ThemeFile = serde_json::from_value(json!({"id":"old","name":"Old","base":"dark","colors":{}})).unwrap();
        assert!(saved.borders.is_empty());
        let partial =
            from_value(&json!({"format":FORMAT,"name":"Partial","base":"dark","borders":{"secondary":{"width":2,"color":"#ffffff"}}})).unwrap();
        assert_eq!(resolve(&partial).primary_border(Color32::RED), egui::Stroke::new(1.0, Color32::RED));
    }

    #[test]
    fn malformed_border_settings_are_refused_and_saved_settings_are_safe() {
        for settings in [
            json!([]),
            json!({"unknown":{"width":1,"color":"#ffffff"}}),
            json!({"primary":true}),
            json!({"primary":{"width":-1,"color":"#ffffff"}}),
            json!({"primary":{"width":4.1,"color":"#ffffff"}}),
            json!({"primary":{"width":"1","color":"#ffffff"}}),
            json!({"primary":{"width":1,"color":"white"}}),
            json!({"primary":{"width":1,"color":"#ffffff","gap":1}}),
            json!({"primary_inset":{"width":1,"color":"#ffffff","gap":13}}),
            json!({"primary_inset":{"width":1,"color":"#ffffff"}}),
        ] {
            assert!(from_value(&json!({"format":FORMAT,"name":"Bad","base":"dark","borders":settings})).is_err(), "{settings}");
        }
        let file = ThemeFile { borders: BTreeMap::from([("primary".into(), json!({"width":1e100,"color":"#ffffff"}))]), ..Default::default() };
        assert_eq!(resolve(&file).borders, crate::theme::Borders::LEGACY);
        assert!(parse_borders(&BTreeMap::from([("primary".into(), json!({"width":null,"color":"#ffffff"}))])).is_err());
        assert_eq!(resolve(find("blueprint", &[]).unwrap()).borders.primary.unwrap().color, Color32::WHITE);
        for id in ["gruvbox-dark", "gruvbox-light", "gruvbox-dark-soft", "gruvbox-light-soft"] {
            assert_eq!(resolve(find(id, &[]).unwrap()).borders, crate::theme::Borders::LEGACY);
        }
    }

    #[test]
    fn bevel_settings_roundtrip_and_reject_invalid_values() {
        let bevel = json!({"width":1,"highlight":"#ffffff","light":"#dfdfdf","shadow":"#808080","dark_shadow":"#000000"});
        for base in ["light", "dark"] {
            let source = json!({"format":FORMAT,"name":"Beveled","base":base,"borders":{"bevel":bevel}});
            let file = from_value(&source).unwrap();
            let tokens = resolve(&file);
            assert_eq!(resolve(&from_value(&export("Beveled", file.base, &tokens)).unwrap()), tokens);
            assert_eq!(tokens.input_fill(), tokens.field);
            for invalid in [json!(0), json!(-1), json!(2.1), json!("1"), Value::Null] {
                let mut bad = source.clone();
                bad["borders"]["bevel"]["width"] = invalid;
                assert!(from_value(&bad).is_err());
            }
            for key in ["highlight", "light", "shadow", "dark_shadow"] {
                let mut bad = source.clone();
                bad["borders"]["bevel"][key] = json!("white");
                assert!(from_value(&bad).is_err());
            }
            let mut bad = source;
            bad["borders"]["bevel"]["extra"] = json!(true);
            assert!(from_value(&bad).is_err());
        }
        for theme in builtin() {
            assert_eq!(resolve(theme).borders.bevel.is_some(), theme.id == "solidcraft-95", "{}", theme.id);
        }
    }

    #[test]
    fn icon_palette_import_export_is_bounded_and_keeps_legacy_defaults() {
        let source = json!({"format":FORMAT,"name":"Icons","base":"dark","icon_palette": {
            "ink":"#123456", "line":"#abcdef", "solid":["#c0d0e0","#456789","#123456"],
            "sketch":["#c0e0d0","#458967","#125634"] }});
        let file = from_value(&source).unwrap();
        let tokens = resolve(&file);
        assert!(tokens.icon_palette.is_some());
        assert_eq!(resolve(&from_value(&export("Icons", file.base, &tokens)).unwrap()), tokens);
        for (key, value) in [
            ("gap", json!("#ffffff")),
            ("unknown", json!("#ffffff")),
            ("ink", json!("#ffffff80")),
            ("solid", json!(["#ffffff"])),
            ("surface", json!(["#ffffff", "#ffffff", "no"])),
        ] {
            let mut bad = source.clone();
            bad["icon_palette"][key] = value;
            assert!(from_value(&bad).is_err(), "{bad}");
        }
        let old = from_value(&json!({"format":FORMAT,"name":"Legacy","base":"light"})).unwrap();
        assert!(resolve(&old).icon_palette.is_none());
        let damaged = ThemeFile { icon_palette: BTreeMap::from([("gap".into(), json!("#ffffff"))]), ..Default::default() };
        assert!(resolve(&damaged).icon_palette.is_none());
    }

    #[test]
    fn shipped_themes_have_readable_distinct_workspace_colours_and_icon_palettes() {
        use crate::theme::{TabAccent, contrast_ratio};
        assert_eq!(builtin().len(), BUILTIN.len(), "all shipped themes must be valid");
        for file in builtin() {
            let t = resolve(file);
            assert!(t.icon_palette.is_some(), "{} has no icon palette", file.id);
            let mut fills = std::collections::HashSet::new();
            for tab in ["MODEL", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER", "SKETCH"] {
                let accent = TabAccent::for_tab(&t, tab);
                assert!(fills.insert(accent.color), "{} repeats {tab}", file.id);
                assert!(contrast_ratio(accent.ink, accent.soft) >= 4.5, "{} {tab} soft text", file.id);
                assert!(contrast_ratio(accent.on_fill(&t), accent.color) >= 4.5, "{} {tab} pill text", file.id);
            }
        }
    }

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
        let t = parse(r##"{"format":"solidcraft-theme/1","name":"My Theme!","base":"light","colors":{"panel":"#102030","axis_z":"#0000ff80"}}"##)
            .unwrap();
        assert_eq!((t.id.as_str(), t.name.as_str(), t.base), ("my-theme", "My Theme!", Base::Light));
        let p = resolve(&t);
        assert_eq!(p.panel, Color32::from_rgb(0x10, 0x20, 0x30));
        assert_eq!(p.axis_z, Color32::from_rgba_unmultiplied(0, 0, 255, 0x80));
        assert_eq!(p.text, Tokens::light().text);
    }

    /// A theme file written before the axes were one set (#976) still reads: its view triad and
    /// Move gizmo lists colour every axis, the triad over the gizmo and `axis_*` over both.
    #[test]
    fn older_triad_and_gizmo_keys_colour_every_axis() {
        let file = |colors: &str| {
            resolve(&parse(&format!(r#"{{"format":"solidcraft-theme/1","name":"Old","base":"dark","colors":{{{colors}}}}}"#)).unwrap())
        };
        let (r, g, b, w) =
            (Color32::from_rgb(0xfb, 0x49, 0x34), Color32::from_rgb(0xb8, 0xbb, 0x26), Color32::from_rgb(0x83, 0xa5, 0x98), Color32::WHITE);
        let triad = r##""triad":["#fb4934","#b8bb26","#83a598"]"##;
        let gizmo = r##""gizmo":["#ffffff","#ffffff","#ffffff"]"##;
        assert_eq!(file(triad).axes(), [r, g, b]);
        assert_eq!(file(gizmo).axes(), [w, w, w]);
        assert_eq!(file(&format!("{gizmo},{triad}")).axes(), [r, g, b], "the triad over the gizmo");
        assert_eq!(file(&format!(r##"{triad},"axis_y":"#ffffff""##)).axes(), [r, w, b], "a file's own axis key wins");
        assert!(!keys().iter().any(|k| LEGACY_AXES.contains(k)), "an export writes axis_x, axis_y and axis_z only");
        let e = parse(r##"{"format":"solidcraft-theme/1","name":"x","base":"dark","colors":{"gizmo":["#000000"]}}"##).unwrap_err();
        assert!(e.contains("`gizmo` must be a list of 3 colours"), "{e}");
    }

    #[test]
    fn older_exports_keep_active_overrides_without_retired_ui_settings() {
        let mut file = export("Older theme", Base::Dark, &Tokens::dark());
        let colors = file.get_mut("colors").and_then(Value::as_object_mut).unwrap();
        colors.insert("panel".into(), json!("#102030"));
        colors.insert("logo".into(), json!(["#102030", "#405060", "#708090", "#000000"]));
        for key in RETIRED_COLORS {
            colors.insert((*key).into(), json!("#abcdef"));
        }
        let imported = from_value(&file).unwrap();
        assert_eq!(resolve(&imported).panel, Color32::from_rgb(0x10, 0x20, 0x30));
        let resolved = resolve(&imported);
        assert_eq!(resolved.logo, [parse_color("#102030").unwrap(), parse_color("#405060").unwrap(), parse_color("#708090").unwrap()]);
        assert_eq!(export("Current", Base::Dark, &resolved)["colors"]["logo"].as_array().unwrap().len(), 3);
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

    fn radius_values(mut tokens: Tokens) -> Vec<u8> {
        RADIUS_KEYS.iter().map(|key| *radius_slot(&mut tokens, key).unwrap()).collect()
    }

    #[test]
    fn optional_radii_preserve_old_files_and_saved_preferences() {
        for base in [Base::Dark, Base::Light] {
            let file = json!({"format": FORMAT, "name": "Old", "base": base});
            let parsed = from_value(&file).unwrap();
            let defaults = if base.is_dark() { Tokens::dark() } else { Tokens::light() };
            assert_eq!(resolve(&parsed), defaults);
            // Preferences serialize ThemeFile directly, without the export format wrapper.
            let stored = json!({"id": "old", "name": "Old", "base": base, "colors": {}});
            let old: ThemeFile = serde_json::from_value(stored).unwrap();
            assert_eq!(resolve(&old), defaults);
            let partial = from_value(&json!({"format": FORMAT, "name": "Partial", "base": base, "radii": {"control_radius": 0}})).unwrap();
            assert_eq!(resolve(&partial), Tokens { control_radius: 0, ..defaults });
        }
    }

    #[test]
    fn every_radius_round_trips_through_export_and_preferences_on_either_base() {
        for values in [[0; 6], [1, 2, 3, 4, 5, 6], [32; 6]] {
            let mut tokens = Tokens::dark();
            for (key, value) in RADIUS_KEYS.iter().zip(values) {
                *radius_slot(&mut tokens, key).unwrap() = value;
            }
            for base in [Base::Dark, Base::Light] {
                let parsed = from_value(&export("Shapes", base, &tokens)).unwrap();
                let restored: ThemeFile = serde_json::from_value(serde_json::to_value(&parsed).unwrap()).unwrap();
                assert_eq!(radius_values(resolve(&restored)), values);
                assert_eq!(restored.radii.len(), RADIUS_KEYS.len());
            }
        }
    }

    #[test]
    fn invalid_radii_are_refused_and_hand_edited_preferences_resolve_safely() {
        for value in [json!(-1), json!(33), json!(256), json!(1.5), json!("4"), json!(null), json!(true), json!([4]), json!({})] {
            let error = from_value(&json!({"format": FORMAT, "name": "Bad", "base": "dark", "radii": {"control_radius": value}})).unwrap_err();
            assert!(error.contains("radii.control_radius"), "{error}");
        }
        assert!(from_value(&json!({"format": FORMAT, "name": "Bad", "base": "dark", "radii": []})).unwrap_err().contains("radii"));
        let error = from_value(&json!({"format": FORMAT, "name": "Bad", "base": "dark", "radii": {"typo": 4, "radius_sm": -1}})).unwrap_err();
        assert!(error.contains("radii.typo") && error.contains("radii.radius_sm"));
        let hand_edited = ThemeFile { radii: BTreeMap::from([("radius_sm".into(), 255), ("typo".into(), 6)]), ..Default::default() };
        assert_eq!(resolve(&hand_edited), Tokens::dark());
    }

    #[test]
    fn built_in_shapes_match_their_character_and_light_dark_families_agree() {
        let values = |id: &str| radius_values(resolve(find(id, &[]).unwrap()));
        for id in ["platinum-9", "green-phosphor", "amber-phosphor"] {
            assert_eq!(values(id), [0; 6], "{id}");
        }
        assert_eq!(values("solidcraft-95"), [3, 3, 4, 4, 3, 3]);
        assert_eq!(values("blueprint"), [4, 6, 8, 10, 6, 7]);
        assert_eq!(values("cupertino"), [6, 8, 12, 16, 8, 9]);
        for id in ["clay", "clay-dark"] {
            assert_eq!(values(id), [6, 9, 13, 18, 9, 10], "{id}");
        }
        let defaults = radius_values(Tokens::dark());
        assert_eq!(defaults, radius_values(Tokens::light()));
        for suffix in ["", "-hard", "-soft"] {
            assert_eq!(values(&format!("gruvbox-dark{suffix}")), defaults);
            assert_eq!(values(&format!("gruvbox-light{suffix}")), defaults);
        }
        for file in builtin().iter().filter(|file| file.radii.is_empty()) {
            assert_eq!(radius_values(resolve(file)), defaults, "{}", file.id);
        }
    }
}
