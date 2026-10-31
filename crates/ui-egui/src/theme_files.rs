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
pub const MAX_BYTES: usize = 256 * 1024;
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

/// A theme: identity, base value tables and optional declarations selecting literal overlays.
/// Colour overrides are `#rrggbb` / `#rrggbbaa` strings or lists, keyed by [`keys`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    /// Optional surface overrides for known ribbon tabs. Accents retain their existing tokens.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tabs: BTreeMap<String, BTreeMap<String, Value>>,
    /// Theme-owned declarations, in file order; standard ids are normalized on import.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<OptionDef>,
    /// Conditional value tables, applied in file order with later entries winning by key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overlays: Vec<Overlay>,
    /// Levels newer than [`REQUIREMENTS_LEVEL`] disable options and overlays.
    #[serde(default = "requirements_level", skip_serializing_if = "is_current_level")]
    pub requires: u64,
}

/// Theme option/overlay format level understood by this app.
pub const REQUIREMENTS_LEVEL: u64 = 1;
pub const MAX_OPTIONS: usize = 16;
pub const MAX_OVERLAYS: usize = 32;
const STANDARD_OPTIONS: [&str; 4] = ["opacity", "blur", "tab_colours", "tab_fade_ms"];

fn requirements_level() -> u64 {
    REQUIREMENTS_LEVEL
}

fn is_current_level(level: &u64) -> bool {
    *level == REQUIREMENTS_LEVEL
}

impl Default for ThemeFile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            base: Base::default(),
            colors: BTreeMap::new(),
            radii: BTreeMap::new(),
            borders: BTreeMap::new(),
            icon_palette: BTreeMap::new(),
            tabs: BTreeMap::new(),
            options: Vec::new(),
            overlays: Vec::new(),
            requires: REQUIREMENTS_LEVEL,
        }
    }
}

/// Saved option values use the same JSON primitives as defaults and comparisons.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OptionValue {
    Bool(bool),
    Choice(String),
    Num(f32),
}

/// Localized text belongs to the theme, with English as the fallback.
pub type OptionText = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChoiceValue {
    pub id: String,
    pub label: OptionText,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum OptionKind {
    Bool {
        default: bool,
    },
    Choice {
        default: String,
        values: Vec<ChoiceValue>,
    },
    Range {
        default: f32,
        min: f32,
        max: f32,
        step: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OptionDef {
    pub id: String,
    #[serde(flatten)]
    pub kind: OptionKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub label: OptionText,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub help: OptionText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub advanced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Condition {
    Eq {
        opt: String,
        eq: OptionValue,
    },
    Range {
        opt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gte: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lte: Option<f32>,
    },
}

impl Condition {
    fn option(&self) -> &str {
        match self {
            Self::Eq { opt, .. } | Self::Range { opt, .. } => opt,
        }
    }
}

/// Overlay tables have exactly the same keys and bounds as the theme's base tables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ThemeLayer {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub radii: BTreeMap<String, u8>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub borders: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub icon_palette: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Overlay {
    pub when: Condition,
    #[serde(flatten)]
    pub values: ThemeLayer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionNoticeKind {
    UnknownOption,
    WrongType,
    UnknownChoice,
    NonFinite,
    Clamped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionNotice {
    pub id: String,
    pub kind: OptionNoticeKind,
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
    /// Shared by import and preference restoration; invalid saved maps are dropped as a unit.
    pub(crate) fn set_tabs(&mut self, source: &Value) -> Result<(), String> {
        self.tabs = parse_tabs(source.get("tabs"))?;
        Ok(())
    }

    /// A newer format level keeps the authored base tables but disables options and overlays.
    /// Computed from `requires` so it also works after restoring saved preferences.
    pub fn needs_newer(&self) -> bool {
        self.requires > REQUIREMENTS_LEVEL
    }

    /// Read option tables from a stored or directly supplied theme with the import parser.
    /// Base tables are deliberately left alone; only completely valid declarations replace these.
    pub(crate) fn set_option_tables(&mut self, source: &Value) -> Result<(), String> {
        let options = if self.needs_newer() { Vec::new() } else { parse_options(source.get("options"))? };
        let overlays = if self.needs_newer() { Vec::new() } else { parse_overlays(source.get("overlays"), &options)? };
        self.options = options;
        self.overlays = overlays;
        Ok(())
    }

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
        nav_hover, nav_press, nav_dim, nav_viewport, nav_tip, nav_tip_ink, nav_tip_key_border, nav_field,
        file_dim, file_recovery_well, file_background, file_surface, file_border, file_image_tint, file_rail, file_rail_ink, file_rail_hover,
        file_primary, file_primary_ink, file_primary_border, file_recovery, file_recovery_ink, file_recovery_border,
        app_bar, toolbar, panel, panel_header, border, text, text_dim, accent, accent_soft, sketch_accent, error, warning,
        icon, timeline, hover, sel_face, sel_edge, sel_edge_rim, hover_edge, hover_edge_halo, sel_profile,
        sel_profile_edge, hover_profile, hover_profile_edge, sel_vertex, origin_plane, origin_plane_edge, origin_plane_hover,
        origin_point, axis_x, axis_y, axis_z, construction_plane, construction_geom, body, body_edge, manipulator, joint_manipulator, joint_axis,
        preview_add, preview_new, preview_sketch, preview_cut, cube_face, cube_border, cube_text, cube_hover, viewport_top,
        viewport_bottom, grid, grid_major, body_selected, sketch, sketch_fixed, sketch_halo, sketch_locked, sketch_conflict, dimension, dimension_driven,
        sketch_point, sketch_point_free, dialog_bg, dialog_border, chip, section_cap,
        profile_fill, glyph_bg, glyph_edge, glyph_text, overlay, rubber_band, app_bar_text, app_bar_hover, timeline_item,
        timeline_item_error, timeline_marker, field, box_window, box_crossing, sketch_projected, sketch_lost,
        sketch_construction, sketch_centerline, bend_up, bend_down, interference,
        sketch_ink, sketch_soft, surface_fill, surface_ink, surface_soft, mesh_fill, mesh_ink, mesh_soft, form_fill, form_tab_ink, form_soft, sheet_fill, sheet_ink, sheet_soft, render_fill, render_ink, render_soft, tab_on_fill_dark,
        app_bar_separator, surface_accent, mesh_accent, form_accent, render_accent, on_accent, agent_ok, agent_busy, app_bar_hover_subtle,
        app_bar_hover_button, app_bar_hover_strong, app_bar_tab_active,
        form_ink, form_dim, form_primary, form_primary_fill, form_error, form_error_fill, form_success, form_success_fill, dialog_body, dialog_footer, dialog_rule, control_fill, control_border, dialog_label, selected_ink, selected_bg,
    ],
    many: [continuity, logo],
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
/// colours, bounded shapes, option definitions and overlay comparisons. Nothing is taken from a
/// supported-level file with any error; messages name the offending key and overlay position.
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
    let requires = match v.get("requires") {
        None => REQUIREMENTS_LEVEL,
        Some(value) => value.as_u64().ok_or("`requires` must be a nonnegative integer")?,
    };
    let layer = parse_layer(v)?;
    let tabs = parse_tabs(v.get("tabs"))?;
    let options = if requires > REQUIREMENTS_LEVEL { Vec::new() } else { parse_options(v.get("options"))? };
    let overlays = if requires > REQUIREMENTS_LEVEL { Vec::new() } else { parse_overlays(v.get("overlays"), &options)? };
    Ok(ThemeFile {
        id: slug(name),
        name: name.to_string(),
        base,
        colors: layer.colors,
        radii: layer.radii,
        borders: layer.borders,
        icon_palette: layer.icon_palette,
        tabs,
        options,
        overlays,
        requires,
    })
}

/// Keep the accepted vocabulary at the same boundary as the existing accent mapping.
fn parse_tabs(v: Option<&Value>) -> Result<BTreeMap<String, BTreeMap<String, Value>>, String> {
    let Some(v) = v else { return Ok(BTreeMap::new()) };
    let tabs = v.as_object().ok_or("`tabs` must be an object")?;
    if tabs.len() > crate::theme::TAB_NAMES.len() {
        return Err("`tabs` has too many entries".into());
    }
    let mut parsed = BTreeMap::new();
    for (tab, surfaces) in tabs {
        if !crate::theme::TAB_NAMES.contains(&tab.as_str()) {
            return Err(format!("`tabs.{tab}` is not a ribbon tab"));
        }
        let surfaces = surfaces.as_object().ok_or_else(|| format!("`tabs.{tab}` must be an object"))?;
        let mut values = BTreeMap::new();
        for (key, value) in surfaces {
            if !matches!(key.as_str(), "panel" | "viewport_top" | "viewport_bottom") {
                return Err(format!("`tabs.{tab}.{key}` is not a tab surface"));
            }
            if value.as_str().and_then(parse_color).is_none() {
                return Err(format!("`tabs.{tab}.{key}` must be #rrggbb or #rrggbbaa"));
            }
            values.insert(key.clone(), value.clone());
        }
        parsed.insert(tab.clone(), values);
    }
    Ok(parsed)
}

fn parse_layer(v: &Value) -> Result<ThemeLayer, String> {
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
    Ok(ThemeLayer { colors: src.into_iter().collect(), radii, borders, icon_palette })
}

fn valid_option_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn option_number(v: &Value, key: &str) -> Result<f32, String> {
    v.get(key)
        .and_then(Value::as_f64)
        .map(|n| n as f32)
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("`{key}` must be a finite number representable as f32"))
}

fn option_text(v: Option<&Value>, key: &str, max: usize) -> Result<OptionText, String> {
    let Some(v) = v else { return Ok(BTreeMap::new()) };
    let obj = v.as_object().ok_or_else(|| format!("`{key}` must be a map of language codes to strings"))?;
    if obj.len() > 64 {
        return Err(format!("`{key}` has more than 64 languages"));
    }
    obj.iter()
        .map(|(language, value)| {
            if language.is_empty() || language.len() > 32 || !language.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
                return Err(format!("`{key}` has an invalid language code"));
            }
            let text = value
                .as_str()
                .filter(|s| s.chars().count() <= max)
                .ok_or_else(|| format!("`{key}.{language}` must be a string of at most {max} characters"))?;
            Ok((language.clone(), text.into()))
        })
        .collect()
}

fn parse_option(v: &Value) -> Result<OptionDef, String> {
    let id = v.get("id").and_then(Value::as_str).filter(|id| valid_option_id(id)).ok_or("`id` must match [a-z][a-z0-9_]{0,31}")?;
    let kind = v.get("type").and_then(Value::as_str).ok_or("`type` must be bool, choice or range")?;
    let reserved = STANDARD_OPTIONS.contains(&id);
    let kind = if id == "tab_colours" || (!reserved && kind == "bool") {
        if kind != "bool" {
            return Err(format!("`{id}.type` must be bool"));
        }
        OptionKind::Bool { default: v.get("default").and_then(Value::as_bool).ok_or("`default` must be bool")? }
    } else if reserved || kind == "range" {
        if kind != "range" {
            return Err(format!("`{id}.type` must be range"));
        }
        let (floor, ceiling, standard_unit) = match id {
            "opacity" => (40.0, 100.0, Some("%")),
            "blur" => (0.0, 40.0, Some("px")),
            "tab_fade_ms" => (0.0, 600.0, Some("ms")),
            _ => (0.0, 0.0, None),
        };
        let min = if reserved && v.get("min").is_none() { floor } else { option_number(v, "min")? };
        let max = if reserved && v.get("max").is_none() { ceiling } else { option_number(v, "max")? };
        if min > max {
            return Err("`min` must not exceed `max`".into());
        }
        if reserved && (min < floor || max > ceiling) {
            return Err(format!("`min` / `max` for {id} must stay inside {floor} to {ceiling}"));
        }
        let step = if id == "tab_fade_ms" {
            20.0
        } else if reserved {
            1.0
        } else {
            option_number(v, "step")?
        };
        if step <= 0.0 {
            return Err("`step` must be greater than zero".into());
        }
        let default = if id == "opacity" && v.get("default").is_none() { 100.0 } else { option_number(v, "default")? };
        let default = if reserved {
            default.clamp(min, max)
        } else {
            if default < min || default > max {
                return Err("`default` must be inside `min` / `max`".into());
            }
            default
        };
        let unit = if reserved {
            standard_unit.map(str::to_string)
        } else {
            v.get("unit")
                .map(|v| v.as_str().filter(|s| s.chars().count() <= 4).map(str::to_string).ok_or("`unit` must be a string of at most 4 characters"))
                .transpose()?
        };
        OptionKind::Range { default, min, max, step, unit }
    } else if kind == "choice" {
        let values = v.get("values").and_then(Value::as_array).filter(|v| (2..=12).contains(&v.len())).ok_or("`values` must have 2 to 12 choices")?;
        let mut ids = std::collections::BTreeSet::new();
        let mut choices = Vec::new();
        for value in values {
            let id = value.get("id").and_then(Value::as_str).filter(|id| valid_option_id(id)).ok_or("`values.id` must match [a-z][a-z0-9_]{0,31}")?;
            if !ids.insert(id) {
                return Err(format!("`values.id` duplicates {id}"));
            }
            let label = option_text(Some(value.get("label").ok_or("`values.label` is required")?), "values.label", 40)?;
            choices.push(ChoiceValue { id: id.into(), label });
        }
        let default = v.get("default").and_then(Value::as_str).filter(|id| ids.contains(id)).ok_or("`default` must name an id in `values`")?;
        OptionKind::Choice { default: default.into(), values: choices }
    } else {
        return Err("`type` must be bool, choice or range".into());
    };
    // Standard definitions are app-owned: even malformed presentation fields are ignored.
    if reserved {
        return Ok(OptionDef {
            id: id.into(),
            kind,
            label: BTreeMap::new(),
            help: BTreeMap::new(),
            parent: (id == "tab_fade_ms").then(|| "tab_colours".into()),
            advanced: false,
            section: None,
        });
    }
    let parent = v
        .get("parent")
        .map(|v| v.as_str().filter(|id| valid_option_id(id)).map(str::to_string).ok_or("`parent` must be an option id"))
        .transpose()?;
    let advanced = v.get("advanced").map(|v| v.as_bool().ok_or("`advanced` must be bool")).transpose()?.unwrap_or(false);
    let section = v
        .get("section")
        .map(|v| v.as_str().filter(|s| s.chars().count() <= 24).map(str::to_string).ok_or("`section` must be a string of at most 24 characters"))
        .transpose()?;
    Ok(OptionDef {
        id: id.into(),
        kind,
        label: option_text(v.get("label"), "label", 40)?,
        help: option_text(v.get("help"), "help", 160)?,
        parent,
        advanced,
        section,
    })
}

fn parse_options(v: Option<&Value>) -> Result<Vec<OptionDef>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let values = v.as_array().filter(|v| v.len() <= MAX_OPTIONS).ok_or("`options` must be an array of at most 16 entries")?;
    let mut options = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for (position, value) in values.iter().enumerate() {
        let option = parse_option(value).map_err(|e| format!("`options[{}]`: {e}", position + 1))?;
        if !ids.insert(option.id.clone()) {
            return Err(format!("`options.id` duplicates {}", option.id));
        }
        options.push(option);
    }
    let mut parents = std::collections::BTreeSet::new();
    for option in &options {
        if let Some(parent) = &option.parent {
            if !options.iter().any(|o| &o.id == parent && matches!(o.kind, OptionKind::Bool { .. })) {
                return Err(format!("`options.{}.parent` must name a bool option", option.id));
            }
            if !parents.insert(parent) {
                return Err(format!("`options.parent` {parent} has more than one child"));
            }
            let mut ancestor = Some(parent.as_str());
            for _ in 0..=MAX_OPTIONS {
                let Some(id) = ancestor else { break };
                if id == option.id {
                    return Err(format!("`options.{}.parent` forms a cycle", option.id));
                }
                ancestor = options.iter().find(|o| o.id == id).and_then(|o| o.parent.as_deref());
            }
        }
    }
    Ok(options)
}

fn parse_condition(v: &Value, options: &[OptionDef]) -> Result<Condition, String> {
    let obj = v.as_object().ok_or("`when` must be an object")?;
    let opt = v.get("opt").and_then(Value::as_str).ok_or("`when.opt` must be an option id")?;
    let option = options.iter().find(|o| o.id == opt).ok_or_else(|| format!("`when.opt` names unknown option {opt}"))?;
    if let Some(eq) = v.get("eq") {
        if obj.len() != 2 {
            return Err("`when.eq` cannot be combined with other comparisons".into());
        }
        let eq = match &option.kind {
            OptionKind::Bool { .. } => OptionValue::Bool(eq.as_bool().ok_or("`when.eq` must be bool")?),
            OptionKind::Choice { values, .. } => OptionValue::Choice(
                eq.as_str().filter(|id| values.iter().any(|v| v.id == *id)).ok_or("`when.eq` must name a choice in `values`")?.into(),
            ),
            OptionKind::Range { .. } => OptionValue::Num(option_number(v, "eq").map_err(|e| format!("`when`: {e}"))?),
        };
        return Ok(Condition::Eq { opt: opt.into(), eq });
    }
    if !matches!(option.kind, OptionKind::Range { .. }) {
        return Err("`when.gte` / `when.lte` require a range option".into());
    }
    if obj.keys().any(|k| !matches!(k.as_str(), "opt" | "gte" | "lte")) {
        return Err("`when` has an unknown comparison".into());
    }
    let bound = |key| v.get(key).map(|_| option_number(v, key).map_err(|e| format!("`when`: {e}"))).transpose();
    let (gte, lte) = (bound("gte")?, bound("lte")?);
    if gte.is_none() && lte.is_none() {
        return Err("`when` needs eq, gte or lte".into());
    }
    if matches!((gte, lte), (Some(a), Some(b)) if a > b) {
        return Err("`when.gte` must not exceed `when.lte`".into());
    }
    Ok(Condition::Range { opt: opt.into(), gte, lte })
}

fn parse_overlays(v: Option<&Value>, options: &[OptionDef]) -> Result<Vec<Overlay>, String> {
    let Some(v) = v else { return Ok(Vec::new()) };
    let values = v.as_array().filter(|v| v.len() <= MAX_OVERLAYS).ok_or("`overlays` must be an array of at most 32 entries")?;
    values
        .iter()
        .enumerate()
        .map(|(position, value)| {
            let parse = || {
                let when = parse_condition(value.get("when").ok_or("`when` is required")?, options)?;
                Ok(Overlay { when, values: parse_layer(value)? })
            };
            parse().map_err(|e: String| format!("`overlays[{}]`: {e}", position + 1))
        })
        .collect()
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

/// Definitions from hand-edited preferences get the same bounds and reserved-id normalization
/// as imports. An invalid definition list cannot drive an overlay.
fn usable_options(theme: &ThemeFile) -> Vec<OptionDef> {
    if theme.needs_newer() || theme.options.len() > MAX_OPTIONS {
        return Vec::new();
    }
    serde_json::to_value(&theme.options).ok().and_then(|v| parse_options(Some(&v)).ok()).unwrap_or_default()
}

impl OptionDef {
    pub fn default_value(&self) -> OptionValue {
        match &self.kind {
            OptionKind::Bool { default } => OptionValue::Bool(*default),
            OptionKind::Choice { default, .. } => OptionValue::Choice(default.clone()),
            OptionKind::Range { default, .. } => OptionValue::Num(*default),
        }
    }
}

/// Values for every declared option. Missing values use defaults without a notice; invalid saved
/// values use defaults and report why. Unknown ids are omitted (there is no definition/default).
/// Range values are clamped, without rounding to `step`. Inactive children retain their values.
pub fn effective_values(theme: &ThemeFile, saved: &BTreeMap<String, OptionValue>) -> (BTreeMap<String, OptionValue>, Vec<OptionNotice>) {
    let options = usable_options(theme);
    let mut values: BTreeMap<_, _> = options.iter().map(|o| (o.id.clone(), o.default_value())).collect();
    let mut notices = Vec::new();
    for (id, saved) in saved {
        let Some(option) = options.iter().find(|o| &o.id == id) else {
            notices.push(OptionNotice { id: id.clone(), kind: OptionNoticeKind::UnknownOption });
            continue;
        };
        let mut notice = None;
        let value = match (&option.kind, saved) {
            (OptionKind::Bool { .. }, OptionValue::Bool(_)) => saved.clone(),
            (OptionKind::Choice { values, .. }, OptionValue::Choice(id)) if values.iter().any(|v| &v.id == id) => saved.clone(),
            (OptionKind::Choice { .. }, OptionValue::Choice(_)) => {
                notice = Some(OptionNoticeKind::UnknownChoice);
                option.default_value()
            }
            (OptionKind::Range { min, max, .. }, OptionValue::Num(n)) if n.is_finite() => {
                let clamped = n.clamp(*min, *max);
                if clamped != *n {
                    notice = Some(OptionNoticeKind::Clamped);
                }
                OptionValue::Num(clamped)
            }
            (OptionKind::Range { .. }, OptionValue::Num(_)) => {
                notice = Some(OptionNoticeKind::NonFinite);
                option.default_value()
            }
            _ => {
                notice = Some(OptionNoticeKind::WrongType);
                option.default_value()
            }
        };
        values.insert(id.clone(), value);
        if let Some(kind) = notice {
            notices.push(OptionNotice { id: id.clone(), kind });
        }
    }
    (values, notices)
}

/// Standard options in the app's fixed order, each followed by its declared child, then custom
/// options in file order. Each definition appears once. No undeclared standard option is added.
pub fn ordered_options(theme: &ThemeFile) -> Vec<&OptionDef> {
    if theme.needs_newer() {
        return Vec::new();
    }
    let options = theme.options.get(..theme.options.len().min(MAX_OPTIONS)).unwrap_or_default();
    let mut ordered: Vec<&OptionDef> = Vec::new();
    for id in ["opacity", "blur", "tab_colours"] {
        let mut next = options.iter().find(|o| o.id == id);
        for _ in 0..MAX_OPTIONS {
            let Some(option) = next else { break };
            if ordered.iter().any(|o| o.id == option.id) {
                break;
            }
            ordered.push(option);
            next = options.iter().find(|o| o.parent.as_deref() == Some(option.id.as_str()));
        }
    }
    for option in options {
        if !ordered.iter().any(|o| o.id == option.id) {
            ordered.push(option);
        }
    }
    ordered
}

fn option_active(id: &str, options: &[OptionDef], values: &BTreeMap<String, OptionValue>) -> bool {
    let mut current = id;
    for _ in 0..=MAX_OPTIONS {
        let Some(option) = options.iter().find(|o| o.id == current) else { return false };
        let Some(parent) = option.parent.as_deref() else { return true };
        if values.get(parent) != Some(&OptionValue::Bool(true)) {
            return false;
        }
        current = parent;
    }
    false
}

fn condition_matches(condition: &Condition, values: &BTreeMap<String, OptionValue>) -> bool {
    match condition {
        Condition::Eq { opt, eq } => values.get(opt) == Some(eq),
        Condition::Range { opt, gte, lte } => matches!(values.get(opt), Some(OptionValue::Num(n))
            if n.is_finite() && gte.is_none_or(|a| *n >= a) && lte.is_none_or(|b| *n <= b)),
    }
}

/// Resolve at declared defaults. Files without options retain the original palette exactly.
pub fn resolve(theme: &ThemeFile) -> Tokens {
    resolve_with(theme, &BTreeMap::new())
}

/// Resolve base tables, then matching overlays in file order. Later tables replace earlier
/// entries by key. Saved values are normalized here too, so callers cannot bypass finite checks.
pub fn resolve_with(theme: &ThemeFile, saved: &BTreeMap<String, OptionValue>) -> Tokens {
    resolve_for_tab(theme, saved, None)
}

/// Resolve a tab and its base/overlays in one pass. Providers run last so opacity also protects
/// tab-authored panels. Restored/direct Rust values are revalidated before they affect tokens.
pub(crate) fn resolve_for_tab(theme: &ThemeFile, saved: &BTreeMap<String, OptionValue>, tab: Option<&str>) -> Tokens {
    if theme.needs_newer() || (theme.options.is_empty() && theme.overlays.is_empty()) || theme.overlays.len() > MAX_OVERLAYS {
        return resolve_base(theme);
    }
    let options = usable_options(theme);
    let overlays = serde_json::to_value(&theme.overlays).ok().and_then(|v| parse_overlays(Some(&v), &options).ok()).unwrap_or_default();
    let (values, _) = effective_values(theme, saved);
    let mut merged = theme.clone();
    for overlay in overlays {
        if !option_active(overlay.when.option(), &options, &values) || !condition_matches(&overlay.when, &values) {
            continue;
        }
        // Legacy axis lists are local to a layer: an overlay's triad must override axes in
        // the base, while explicit axis keys in that same overlay still win over its triad.
        for key in LEGACY_AXES {
            if let Some(Value::Array(colors)) = overlay.values.colors.get(key) {
                for (axis, color) in ["axis_x", "axis_y", "axis_z"].into_iter().zip(colors) {
                    merged.colors.insert(axis.into(), color.clone());
                }
            }
        }
        merged.colors.extend(overlay.values.colors);
        merged.radii.extend(overlay.values.radii);
        merged.borders.extend(overlay.values.borders);
        merged.icon_palette.extend(overlay.values.icon_palette);
    }
    let mut tokens = resolve_base(&merged);
    let mut tab_panel = false;
    if crate::theme_options::tab_settings(theme, &values).0
        && let Some(tab) = tab
        && let Ok(tabs) = serde_json::to_value(&theme.tabs).map_err(|e| e.to_string()).and_then(|v| parse_tabs(Some(&v)))
        && let Some(surfaces) = tabs.get(tab)
    {
        for (key, value) in surfaces {
            if let Some(slot) = slot(&mut tokens, key)
                && let Ok(colors) = colors_for_key(key, value, 1)
            {
                slot.copy_from_slice(&colors);
            }
        }
        tab_panel = surfaces.contains_key("panel");
    }
    crate::theme_options::apply(theme, &values, &mut tokens);
    if tab_panel {
        tokens.protect_tab_panel();
    }
    tokens
}

/// A colour the saved preferences hold that this version cannot read is skipped safely.
fn resolve_base(theme: &ThemeFile) -> Tokens {
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
    if !theme.colors.contains_key("form_primary_fill") {
        t.form_primary_fill = t.form_primary;
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
    // Conflicting sketch geometry is drawn in the theme's own error colour unless it sets one of
    // its own (#7244).
    if !theme.colors.contains_key("sketch_conflict") {
        t.sketch_conflict = t.error;
    }
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

/// Export a theme definition, preserving its options and overlays rather than flattening them
/// to the current tokens. The existing [`export`] remains a complete token snapshot.
pub fn export_file(theme: &ThemeFile) -> Value {
    let mut value = json!({"format": FORMAT, "name": theme.name, "base": theme.base, "colors": theme.colors});
    if let Some(obj) = value.as_object_mut() {
        if !theme.radii.is_empty() {
            obj.insert("radii".into(), json!(theme.radii));
        }
        if !theme.borders.is_empty() {
            obj.insert("borders".into(), json!(theme.borders));
        }
        if !theme.icon_palette.is_empty() {
            obj.insert("icon_palette".into(), json!(theme.icon_palette));
        }
        if !theme.tabs.is_empty() {
            obj.insert("tabs".into(), json!(theme.tabs));
        }
        if !theme.options.is_empty() {
            obj.insert("options".into(), json!(theme.options));
        }
        if !theme.overlays.is_empty() {
            obj.insert("overlays".into(), json!(theme.overlays));
        }
        if !is_current_level(&theme.requires) {
            obj.insert("requires".into(), json!(theme.requires));
        }
    }
    value
}

// Theme sources are collected at build time; picker order is validated below.
include!(concat!(env!("OUT_DIR"), "/builtin_themes.rs"));

/// The two-colour built-in themes (1-bit and 1-bit Dark): pure black and white, so a few looks
/// other themes tell apart by colour are told apart by text or shape instead.
#[cfg(test)]
pub(crate) fn is_two_colour(id: &str) -> bool {
    matches!(id, "1-bit" | "1-bit-dark")
}

#[cfg(test)]
pub(crate) fn tab_fixture(base: &str) -> ThemeFile {
    let catalog: Value = serde_json::from_str(include_str!("../tests/fixtures/theme-tabs.json")).unwrap();
    from_value(&catalog[base]).unwrap()
}

/// The built-in themes, validated and ordered once; a test checks every shipped file.
pub fn builtin() -> &'static [ThemeFile] {
    static THEMES: OnceLock<Vec<ThemeFile>> = OnceLock::new();
    THEMES.get_or_init(|| match read_builtin(BUILTIN) {
        Ok(themes) => themes,
        Err(e) => {
            log::error!("built-in themes: {e}");
            Vec::new()
        }
    })
}

fn read_builtin(sources: &[(&str, &str)]) -> Result<Vec<ThemeFile>, String> {
    let mut orders = std::collections::BTreeSet::new();
    let mut ids = std::collections::BTreeSet::new();
    let mut themes = Vec::new();
    for &(id, text) in sources {
        if !ids.insert(id) || id.is_empty() {
            return Err(format!("empty or duplicate theme id: {id}"));
        }
        if text.len() > MAX_BYTES {
            return Err(format!("theme {id} exceeds the file size limit"));
        }
        let value: Value = serde_json::from_str(text).map_err(|e| format!("theme {id}: {e}"))?;
        let order = value.get("order").and_then(Value::as_u64).ok_or_else(|| format!("theme {id}: order must be a nonnegative integer"))?;
        if !orders.insert(order) {
            return Err(format!("theme {id}: duplicate order {order}"));
        }
        if value.get("provenance").and_then(Value::as_str).is_none_or(|s| s.trim().is_empty()) {
            return Err(format!("theme {id}: missing provenance"));
        }
        let theme = from_value(&value).map_err(|e| format!("theme {id}: {e}"))?;
        themes.push((order, ThemeFile { id: id.into(), ..theme }));
    }
    if themes.is_empty() {
        return Err("no built-in themes".into());
    }
    themes.sort_by_key(|(order, _)| *order);
    Ok(themes.into_iter().map(|(_, theme)| theme).collect())
}

/// The theme a `theme` preference names: built in, or imported. `None` for dark, light, system
/// or an unknown id.
pub fn find<'a>(id: &str, custom: &'a [ThemeFile]) -> Option<&'a ThemeFile> {
    builtin().iter().find(|t| t.id == id).or_else(|| custom.iter().find(|t| t.id == id))
}

/// Retired built-in cards now select a contrast value on their light/dark family.
/// Keep these ids reserved so importing a theme cannot take a saved built-in identity.
pub(crate) const GRUVBOX_ALIASES: [(&str, &str, &str); 4] = [
    ("gruvbox-dark-hard", "gruvbox-dark", "hard"),
    ("gruvbox-dark-soft", "gruvbox-dark", "soft"),
    ("gruvbox-light-hard", "gruvbox-light", "hard"),
    ("gruvbox-light-soft", "gruvbox-light", "soft"),
];

pub(crate) fn legacy_gruvbox(id: &str) -> Option<(&'static str, &'static str)> {
    GRUVBOX_ALIASES.iter().find(|(old, _, _)| *old == id).map(|(_, family, contrast)| (*family, *contrast))
}

/// Add an imported theme to `custom`, replacing one with the same id (importing a file again
/// updates it). Its id never takes a built-in one's or the modes' (dark, light, system).
/// Returns the id it is kept under.
pub fn add(custom: &mut Vec<ThemeFile>, mut theme: ThemeFile) -> Result<String, String> {
    if ["dark", "light", "system"].contains(&theme.id.as_str()) || builtin().iter().any(|t| t.id == theme.id) || legacy_gruvbox(&theme.id).is_some() {
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

/// Seconds between two looks at a followed file's modified time and size.
pub const FOLLOW_INTERVAL: f64 = 1.0;
/// Longest followed path accepted, in bytes (hostile input from the control channel).
pub const MAX_FOLLOW_PATH: usize = 4096;

/// A theme file the desktop app follows (#7573): another tool writes it, and the app reloads it
/// when its modified time or size changes. Only a read that parses completely is applied; while
/// the file is being written, broken, missing or too big, the last good theme stays on and
/// [`Follow::error`] says why.
#[derive(Clone, Debug, Default)]
pub struct Follow {
    /// The `--theme-file` launch flag's path: followed for this run in place of the preference,
    /// never saved.
    pub launch: Option<String>,
    /// The path the state below belongs to.
    path: String,
    /// The file's modified time and size at the last read.
    stamp: Option<(Option<std::time::SystemTime>, u64)>,
    /// When (`egui` input time, seconds) the file was last looked at.
    checked: Option<f64>,
    /// The last theme that read completely.
    theme: Option<ThemeFile>,
    /// Why the last read was not applied.
    error: Option<String>,
    /// Whether the window had the focus last frame (gaining it re-reads the file).
    pub focused: bool,
}

/// What a [`Follow::poll`] changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FollowChange {
    /// Nothing new.
    None,
    /// The file read completely; its theme is on now.
    Loaded(String),
    /// A read failed for a new reason (reported once; the last good theme stays on).
    Failed(String),
}

/// A followed path as given: trimmed, at most [`MAX_FOLLOW_PATH`] bytes, no control characters.
pub fn follow_path(path: &str) -> Result<String, String> {
    let path = path.trim();
    if path.len() > MAX_FOLLOW_PATH || path.chars().any(char::is_control) {
        return Err(format!("a followed theme file's path is at most {MAX_FOLLOW_PATH} bytes without control characters"));
    }
    Ok(path.to_string())
}

impl Follow {
    /// The path followed now: the launch flag's, else the preference's (empty: none).
    pub fn path<'a>(&'a self, preference: &'a str) -> &'a str {
        match self.launch.as_deref().map(str::trim) {
            Some(path) if !path.is_empty() => path,
            _ => preference.trim(),
        }
    }

    /// The theme the followed file gave last, if any read completely.
    pub fn theme(&self) -> Option<&ThemeFile> {
        self.theme.as_ref()
    }

    /// Why the followed file's last read was not applied.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Look at `path` (`now` in seconds): at most every [`FOLLOW_INTERVAL`] unless `force`, and
    /// read it only when its modified time or size changed. An empty path stops following.
    pub fn poll(&mut self, path: &str, now: f64, force: bool) -> FollowChange {
        if path != self.path {
            *self = Follow { launch: self.launch.take(), focused: self.focused, path: path.to_string(), ..Default::default() };
        }
        if path.is_empty() {
            return FollowChange::None;
        }
        let due = self.checked.is_none_or(|t| now - t >= FOLLOW_INTERVAL || now < t);
        if !force && !due {
            return FollowChange::None;
        }
        self.checked = Some(now);
        match self.read(force) {
            Ok(None) => FollowChange::None,
            Ok(Some(theme)) => {
                let name = theme.name.clone();
                self.theme = Some(theme);
                self.error = None;
                FollowChange::Loaded(name)
            }
            Err(e) if self.error.as_deref() == Some(e.as_str()) => FollowChange::None,
            Err(e) => {
                self.error = Some(e.clone());
                FollowChange::Failed(e)
            }
        }
    }

    /// The file's theme when it changed since the last read (`Ok(None)`: unchanged).
    fn read(&mut self, force: bool) -> Result<Option<ThemeFile>, String> {
        use std::io::Read;
        let path = std::path::Path::new(&self.path);
        let meta = std::fs::metadata(path).map_err(|e| format!("can't read {}: {e}", self.path))?;
        if !meta.is_file() {
            return Err(format!("{} is not a file", self.path));
        }
        if meta.len() > MAX_BYTES as u64 {
            return Err(format!("a theme file is at most {} KiB", MAX_BYTES / 1024));
        }
        let stamp = (meta.modified().ok(), meta.len());
        if !force && self.stamp == Some(stamp) {
            return Ok(None);
        }
        self.stamp = Some(stamp);
        // Never more than the cap, whatever the size said a moment ago.
        let mut text = String::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_BYTES as u64 + 1).read_to_string(&mut text))
            .map_err(|e| format!("can't read {}: {e}", self.path))?;
        let theme = parse(&text)?;
        Ok(Some(ThemeFile { id: format!("file-{}", theme.id), ..theme }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_import_is_strict_and_roundtrips_authored_surfaces() {
        for base in ["dark", "light"] {
            let file = tab_fixture(base);
            assert_eq!(from_value(&export_file(&file)).unwrap(), file);
            let mut source = export_file(&file);
            source["tabs"] = json!(crate::theme::TAB_NAMES.iter().map(|tab| (*tab, json!({"panel":"#18202aff"}))).collect::<BTreeMap<_, _>>());
            assert_eq!(from_value(&source).unwrap().tabs.len(), crate::theme::TAB_NAMES.len());
            for (tabs, offending) in [
                (json!(null), "tabs"),
                (json!([]), "tabs"),
                (json!({"sketch":{}}), "sketch"),
                (json!({"MISSING":{}}), "MISSING"),
                (json!({"SKETCH":null}), "SKETCH"),
                (json!({"SKETCH":{"panel":"#bad"}}), "panel"),
                (json!({"SKETCH":{"panel":["#ffffff"]}}), "panel"),
                (json!({"SKETCH":{"panel":12}}), "panel"),
            ] {
                source["tabs"] = tabs;
                assert!(from_value(&source).unwrap_err().contains(offending));
            }
            // Refuse every known non-surface token, not only status colours.
            for key in keys().into_iter().filter(|key| !matches!(*key, "panel" | "viewport_top" | "viewport_bottom")) {
                source["tabs"] = json!({"SKETCH":{key:"#ffffff"}});
                assert!(from_value(&source).unwrap_err().contains(key), "{key}");
            }
            source["tabs"] = json!({"SKETCH":{"viewport_top":"#12345678"}});
            let file = from_value(&source).unwrap();
            assert_eq!(export_file(&file)["tabs"], source["tabs"]);
        }
    }

    #[test]
    fn tabs_resolve_only_surfaces_and_off_is_exactly_base() {
        for base in ["dark", "light"] {
            let file = tab_fixture(base);
            let defaults = BTreeMap::new();
            let base = resolve_with(&file, &defaults);
            for tab in crate::theme::TAB_NAMES {
                let mut want = base;
                if let Some(surfaces) = file.tabs.get(*tab) {
                    for (key, value) in surfaces {
                        slot(&mut want, key).unwrap().copy_from_slice(&colors_of(value, 1).unwrap());
                    }
                }
                let effective = resolve_for_tab(&file, &defaults, Some(tab));
                assert_eq!(effective, want, "{tab}");
                assert_eq!(crate::theme::TabAccent::for_tab(&effective, tab), crate::theme::TabAccent::for_tab(&base, tab));
                let off = BTreeMap::from([("tab_colours".into(), OptionValue::Bool(false))]);
                assert_eq!(resolve_for_tab(&file, &off, Some(tab)), resolve_with(&file, &off));
            }
            let mut damaged = file.clone();
            damaged.tabs.get_mut("SKETCH").unwrap().insert("error".into(), json!("#ffffff"));
            assert_eq!(resolve_for_tab(&damaged, &defaults, Some("SKETCH")), base);
            let mut absent = file.clone();
            absent.tabs.clear();
            assert_eq!(resolve_for_tab(&absent, &defaults, Some("SKETCH")), base);
            assert_eq!(export_file(&file)["options"][1]["default"], 240.0);
        }
        for file in builtin() {
            assert!(file.tabs.is_empty(), "{}", file.id);
            assert_eq!(resolve_for_tab(file, &BTreeMap::new(), Some("SKETCH")), resolve(file));
        }
    }

    fn option_theme(options: Value, overlays: Value) -> Value {
        json!({"format":FORMAT,"name":"Options","base":"dark","options":options,"overlays":overlays})
    }

    fn sample_options() -> Value {
        json!([
            {"id":"enabled","type":"bool","default":true,"label":{"en":"Enable","da":"Aktivér"},"help":{"en":"Theme-owned help"}},
            {"id":"style","type":"choice","default":"warm","parent":"enabled","advanced":true,"section":"Appearance",
                "values":[{"id":"warm","label":{"en":"Warm"}},{"id":"cool","label":{"en":"Cool"}}]},
            {"id":"amount","type":"range","default":5,"min":0,"max":10,"step":0.5,"unit":"pt"}
        ])
    }

    #[test]
    fn option_definitions_reject_bad_keys_and_hostile_numbers() {
        let mut cases = vec![
            (json!([{"id":"Bad","type":"bool","default":true}]), "id"),
            (json!([{"id":"a","type":"script","default":true}]), "type"),
            (json!([{"id":"a","type":"bool","default":1}]), "default"),
            (json!([{"id":"a","type":"bool","default":true},{"id":"a","type":"bool","default":false}]), "id"),
            (json!([{"id":"opacity","type":"bool","default":true}]), "type"),
            (json!([{"id":"tab_colours","type":"range","default":1}]), "type"),
            (json!([{"id":"a","type":"choice","default":"missing","values":[{"id":"x","label":{}},{"id":"y","label":{}}]}]), "default"),
            (json!([{"id":"a","type":"choice","default":"x","values":[{"id":"x","label":{}},{"id":"x","label":{}}]}]), "values.id"),
            (json!([{"id":"a","type":"choice","default":"x","values":[{"id":"x","label":{}}]}]), "values"),
            (json!([{"id":"a","type":"range","default":1,"min":2,"max":1,"step":1}]), "min"),
            (json!([{"id":"a","type":"range","default":3,"min":0,"max":2,"step":1}]), "default"),
            (json!([{"id":"a","type":"range","default":1,"min":0,"max":2,"step":0}]), "step"),
            (json!([{"id":"a","type":"range","default":1,"min":0,"max":2,"step":-1}]), "step"),
            (json!([{"id":"a","type":"range","default":1,"min":0,"max":2,"step":1,"unit":"points"}]), "unit"),
            (json!([{"id":"a","type":"bool","default":true,"label":{"en":"x".repeat(41)}}]), "label"),
            (json!([{"id":"a","type":"bool","default":true,"help":{"en":"x".repeat(161)}}]), "help"),
            (json!([{"id":"a","type":"bool","default":true,"section":"x".repeat(25)}]), "section"),
            (json!([{"id":"a".repeat(33),"type":"bool","default":true}]), "id"),
            (json!([{"id":"a","type":"bool","default":true,"parent":"missing"}]), "parent"),
            (json!([{"id":"a","type":"bool","default":true,"advanced":"yes"}]), "advanced"),
            (json!([{"id":"opacity","type":"range","default":80,"min":39}]), "min"),
            (json!([{"id":"blur","type":"range","default":10,"max":41}]), "max"),
        ];
        let mut too_many = Vec::new();
        for i in 0..17 {
            too_many.push(json!({"id":format!("o{i}"),"type":"bool","default":true}));
        }
        cases.push((json!(too_many), "options"));
        let mut too_many_choices = sample_options();
        too_many_choices[1]["values"] = json!((0..13).map(|i| json!({"id":format!("v{i}"),"label":{}})).collect::<Vec<_>>());
        cases.push((too_many_choices, "values"));
        for key in ["min", "max", "step", "default"] {
            // JSON cannot encode NaN/infinity; null, strings and finite f64 values overflowing
            // f32 must still name the precise field. Actual non-finite Rust values are below.
            for value in [Value::Null, json!("NaN"), json!("Infinity"), json!(1e100), json!(-1e100)] {
                let mut options = sample_options();
                options[2][key] = value;
                cases.push((options, key));
            }
        }
        for (options, key) in cases {
            let error = from_value(&option_theme(options, json!([]))).unwrap_err();
            assert!(error.contains(key), "expected {key}: {error}");
        }
        for number in ["NaN", "Infinity", "-Infinity", "1e400"] {
            let text = format!(
                r#"{{"format":"{FORMAT}","name":"Bad","base":"dark","options":[{{"id":"x","type":"range","min":0,"max":10,"step":1,"default":{number}}}]}}"#
            );
            assert!(parse(&text).is_err(), "{number} is not a JSON finite number");
        }
    }

    #[test]
    fn option_parents_must_be_unique_bool_parents_without_cycles() {
        let mut options = sample_options();
        options[1]["parent"] = json!("amount");
        assert!(from_value(&option_theme(options, json!([]))).unwrap_err().contains("parent"));
        let mut options = sample_options();
        options[2]["parent"] = json!("enabled");
        assert!(from_value(&option_theme(options, json!([]))).unwrap_err().contains("parent"));
        for options in [
            json!([{"id":"a","type":"bool","default":true,"parent":"a"}]),
            json!([{"id":"a","type":"bool","default":true,"parent":"b"},{"id":"b","type":"bool","default":true,"parent":"a"}]),
            json!([{"id":"tab_fade_ms","type":"range","default":100}]),
        ] {
            assert!(from_value(&option_theme(options, json!([]))).unwrap_err().contains("parent"));
        }
    }

    #[test]
    fn option_and_overlay_limits_accept_their_boundaries_and_count_unicode_characters() {
        let mut options = sample_options().as_array().unwrap().clone();
        options[0]["label"] = json!({"en":"é".repeat(40)});
        options[0]["help"] = json!({"en":"é".repeat(160)});
        options[0]["section"] = json!("é".repeat(24));
        options[1]["values"] = json!((0..12).map(|i| json!({"id":format!("v{i}"),"label":{"en":"é".repeat(40)}})).collect::<Vec<_>>());
        options[1]["default"] = json!("v11");
        options[2]["unit"] = json!("é".repeat(4));
        for i in options.len()..MAX_OPTIONS {
            options.push(json!({"id":format!("o{i}"),"type":"bool","default":false}));
        }
        let overlays = (0..MAX_OVERLAYS).map(|i| json!({"when":{"opt":"enabled","eq":true},"radii":{"radius_sm":i}})).collect::<Vec<_>>();
        let theme = from_value(&option_theme(json!(options), json!(overlays))).unwrap();
        assert_eq!((theme.options.len(), theme.overlays.len()), (MAX_OPTIONS, MAX_OVERLAYS));
        assert_eq!(resolve(&theme).radius_sm, 31);
        assert_eq!(from_value(&export_file(&theme)).unwrap(), theme);
    }

    #[test]
    fn reserved_options_are_app_owned_and_ordered_with_their_children() {
        let file = from_value(&option_theme(
            json!([
                {"id":"custom_first","type":"bool","default":false},
                {"id":"tab_fade_ms","type":"range","default":9999,"min":100,"max":500,"unit":"bad","parent":"wrong","step":null},
                {"id":"tab_colours","type":"bool","default":true,"advanced":"ignored","label":null},
                {"id":"blur","type":"range","default":-5,"help":false,"section":42},
                {"id":"custom_child","type":"choice","default":"a","parent":"tab_fade_ms",
                    "values":[{"id":"a","label":{}},{"id":"b","label":{}}]},
                {"id":"opacity","type":"range","default":200,"min":60,"max":90,"step":0,"unit":false}
            ]),
            json!([]),
        ))
        .unwrap_err();
        assert!(file.contains("parent"), "a reserved range cannot be a bool parent");
        let file = from_value(&option_theme(
            json!([
                {"id":"custom_first","type":"bool","default":false},
                {"id":"tab_fade_ms","type":"range","default":9999,"min":100,"max":500,"unit":"bad","parent":"wrong","step":null},
                {"id":"tab_colours","type":"bool","default":true,"advanced":"ignored","label":null},
                {"id":"blur","type":"range","default":-5,"help":false,"section":42},
                {"id":"custom_child","type":"bool","default":true,"parent":"custom_first"},
                {"id":"opacity","type":"range","default":200,"min":60,"max":90,"step":0,"unit":false}
            ]),
            json!([]),
        ))
        .unwrap();
        let (values, notices) = effective_values(&file, &BTreeMap::new());
        assert!(notices.is_empty());
        assert_eq!(values["opacity"], OptionValue::Num(90.0));
        assert_eq!(values["blur"], OptionValue::Num(0.0));
        assert_eq!(values["tab_fade_ms"], OptionValue::Num(500.0));
        let fade = file.options.iter().find(|o| o.id == "tab_fade_ms").unwrap();
        assert_eq!(fade.parent.as_deref(), Some("tab_colours"));
        assert!(matches!(&fade.kind, OptionKind::Range { min:100.0, max:500.0, step:20.0, unit:Some(unit), .. } if unit == "ms"));
        assert!(
            file.options
                .iter()
                .filter(|o| STANDARD_OPTIONS.contains(&o.id.as_str()))
                .all(|o| o.label.is_empty() && o.help.is_empty() && !o.advanced && o.section.is_none())
        );
        assert_eq!(
            ordered_options(&file).iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            ["opacity", "blur", "tab_colours", "tab_fade_ms", "custom_first", "custom_child"]
        );
        let child = from_value(&option_theme(
            json!([
                {"id":"child","type":"bool","default":false,"parent":"tab_colours"},
                {"id":"other","type":"bool","default":false},
                {"id":"tab_colours","type":"bool","default":true}
            ]),
            json!([]),
        ))
        .unwrap();
        assert_eq!(ordered_options(&child).iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["tab_colours", "child", "other"]);
    }

    #[test]
    fn effective_option_values_fall_back_clamp_and_report_each_saved_problem() {
        let theme = from_value(&option_theme(sample_options(), json!([]))).unwrap();
        let defaults = effective_values(&theme, &BTreeMap::new());
        assert!(defaults.1.is_empty());
        for (saved, expected, kind) in [
            (OptionValue::Choice("typo".into()), OptionValue::Choice("warm".into()), OptionNoticeKind::UnknownChoice),
            (OptionValue::Bool(false), OptionValue::Choice("warm".into()), OptionNoticeKind::WrongType),
        ] {
            let (values, notices) = effective_values(&theme, &BTreeMap::from([("style".into(), saved)]));
            assert_eq!(values["style"], expected);
            assert_eq!(notices, [OptionNotice { id: "style".into(), kind }]);
        }
        for (n, expected, kind) in [
            (-1.0, 0.0, OptionNoticeKind::Clamped),
            (20.0, 10.0, OptionNoticeKind::Clamped),
            (f32::NAN, 5.0, OptionNoticeKind::NonFinite),
            (f32::INFINITY, 5.0, OptionNoticeKind::NonFinite),
            (f32::NEG_INFINITY, 5.0, OptionNoticeKind::NonFinite),
        ] {
            let (values, notices) = effective_values(&theme, &BTreeMap::from([("amount".into(), OptionValue::Num(n))]));
            assert_eq!(values["amount"], OptionValue::Num(expected));
            assert_eq!(notices, [OptionNotice { id: "amount".into(), kind }]);
        }
        let (values, notices) = effective_values(
            &theme,
            &BTreeMap::from([
                ("enabled".into(), OptionValue::Choice("bad".into())),
                ("style".into(), OptionValue::Choice("bad".into())),
                ("amount".into(), OptionValue::Num(99.0)),
                ("removed".into(), OptionValue::Bool(true)),
            ]),
        );
        assert_eq!(notices.len(), 4);
        assert!(!values.contains_key("removed"));
        assert_eq!(values["enabled"], OptionValue::Bool(true));
        let saved = BTreeMap::from([
            ("enabled".into(), OptionValue::Bool(false)),
            ("style".into(), OptionValue::Choice("cool".into())),
            ("amount".into(), OptionValue::Num(4.25)),
        ]);
        assert_eq!(effective_values(&theme, &saved), (saved, Vec::new()), "step is a UI increment, not quantization");
    }

    #[test]
    fn overlays_reject_unknown_options_types_comparisons_and_invalid_tables() {
        let mut cases = vec![
            (json!({"opt":"missing","eq":true}), json!({}), "opt"),
            (json!({"opt":"enabled","eq":1}), json!({}), "eq"),
            (json!({"opt":"style","eq":"missing"}), json!({}), "eq"),
            (json!({"opt":"amount","eq":false}), json!({}), "eq"),
            (json!({"opt":"enabled","gte":0}), json!({}), "gte"),
            (json!({"opt":"style","lte":10}), json!({}), "lte"),
            (json!({"opt":"amount","gte":9,"lte":2}), json!({}), "gte"),
            (json!({"opt":"amount"}), json!({}), "when"),
            (json!({"opt":"amount","eq":2,"gte":0}), json!({}), "eq"),
            (json!({"opt":"amount","gt":2}), json!({}), "when"),
            (json!({"opt":"enabled","eq":true}), json!({"colors":{"panle":"#000000"}}), "panle"),
            (json!({"opt":"enabled","eq":true}), json!({"radii":{"radius_sm":33}}), "radii.radius_sm"),
            (json!({"opt":"enabled","eq":true}), json!({"borders":{"primary":{"width":5,"color":"#ffffff"}}}), "borders.primary.width"),
            (json!({"opt":"enabled","eq":true}), json!({"icon_palette":{"ink":"#ffffff80"}}), "icon_palette.ink"),
        ];
        for key in ["eq", "gte", "lte"] {
            cases.push((json!({"opt":"amount",key:1e100}), json!({}), key));
        }
        for (when, mut overlay, key) in cases {
            overlay["when"] = when;
            let error = from_value(&option_theme(
                sample_options(),
                json!([
                    {"when":{"opt":"enabled","eq":false}}, overlay
                ]),
            ))
            .unwrap_err();
            assert!(error.contains("overlays[2]") && error.contains(key), "{key}: {error}");
        }
        let overlays = vec![json!({"when":{"opt":"enabled","eq":true}}); 33];
        assert!(from_value(&option_theme(sample_options(), json!(overlays))).unwrap_err().contains("overlays"));
    }

    #[test]
    fn matching_overlays_apply_in_file_order_and_preserve_other_table_entries() {
        let mut source = option_theme(
            sample_options(),
            json!([
                {"when":{"opt":"enabled","eq":true},"colors":{"panel":"#123456","axis_x":"#111111"},"radii":{"radius_sm":1},
                    "borders":{"primary":{"width":2,"color":"#abcdef"}},"icon_palette":{"ink":"#112233"}},
                {"when":{"opt":"amount","gte":5,"lte":5},"colors":{"panel":"#abcdef","triad":["#222222","#333333","#444444"]},
                    "radii":{"radius_md":2},"icon_palette":{"line":"#445566"}},
                {"when":{"opt":"amount","eq":6},"colors":{"panel":"#000000"}},
                {"when":{"opt":"style","eq":"cool"},"colors":{"panel":"#ffffff"}}
            ]),
        );
        source["colors"] = json!({"text":"#fedcba","axis_x":"#000000"});
        source["radii"] = json!({"radius_lg":3});
        source["borders"] = json!({"control":{"width":1,"color":"#123456"}});
        source["icon_palette"] = json!({"solid":["#ffffff","#777777","#000000"]});
        let theme = from_value(&source).unwrap();
        let resolved = resolve(&theme);
        assert_eq!(resolved.panel, parse_color("#abcdef").unwrap());
        assert_eq!(resolved.text, parse_color("#fedcba").unwrap());
        assert_eq!(resolved.axes(), [parse_color("#222222").unwrap(), parse_color("#333333").unwrap(), parse_color("#444444").unwrap()]);
        assert_eq!((resolved.radius_sm, resolved.radius_md, resolved.radius_lg), (1, 2, 3));
        assert_eq!(resolved.borders.primary.unwrap().width, 2.0);
        assert_eq!(resolved.borders.control.unwrap().width, 1.0);
        let palette = export_icon_palette(resolved.icon_palette);
        assert_eq!(palette["ink"], json!("#112233"));
        assert_eq!(palette["line"], json!("#445566"));
        assert_eq!(palette["solid"], source["icon_palette"]["solid"]);
        assert_eq!(resolve_with(&theme, &BTreeMap::from([("amount".into(), OptionValue::Num(6.0))])).panel, Color32::BLACK);
        let no_matches = BTreeMap::from([("enabled".into(), OptionValue::Bool(false)), ("amount".into(), OptionValue::Num(1.0))]);
        assert_eq!(resolve_with(&theme, &no_matches), resolve_base(&theme));
        assert_eq!(resolve_with(&theme, &effective_values(&theme, &BTreeMap::new()).0), resolved);
    }

    #[test]
    fn inactive_children_never_match_overlays_and_values_resume_when_parent_is_on() {
        let theme = from_value(&option_theme(
            sample_options(),
            json!([
                {"when":{"opt":"style","eq":"cool"},"colors":{"panel":"#123456"}}
            ]),
        ))
        .unwrap();
        let mut saved = BTreeMap::from([("enabled".into(), OptionValue::Bool(false)), ("style".into(), OptionValue::Choice("cool".into()))]);
        let effective = effective_values(&theme, &saved).0;
        assert_eq!(effective["style"], OptionValue::Choice("cool".into()));
        assert_eq!(resolve_with(&theme, &effective), resolve_base(&theme));
        saved.insert("enabled".into(), OptionValue::Bool(true));
        assert_eq!(resolve_with(&theme, &saved).panel, parse_color("#123456").unwrap());
    }

    #[test]
    fn options_and_overlays_roundtrip_without_changing_old_export_shape() {
        let theme = from_value(&option_theme(
            sample_options(),
            json!([
                {"when":{"opt":"amount","gte":4},"colors":{"panel":"#123456"}},
                {"when":{"opt":"style","eq":"warm"},"radii":{"radius_sm":0}}
            ]),
        ))
        .unwrap();
        let exported = export_file(&theme);
        assert_eq!(from_value(&exported).unwrap(), theme);
        assert_eq!(parse(&exported.to_string()).unwrap(), theme);
        assert_eq!(serde_json::from_value::<ThemeFile>(serde_json::to_value(&theme).unwrap()).unwrap(), theme);
        let old = from_value(&json!({"format":FORMAT,"name":"Old","base":"light"})).unwrap();
        assert_eq!(from_value(&export_file(&old)).unwrap(), old);
        for value in [export_file(&old), serde_json::to_value(&old).unwrap(), export("Snapshot", theme.base, &resolve(&theme))] {
            for key in ["options", "overlays", "requires"] {
                assert!(value.get(key).is_none(), "{key}: {value}");
            }
        }
        let snapshot = export("Snapshot", theme.base, &resolve(&theme));
        assert_eq!(
            snapshot.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(),
            ["base", "borders", "colors", "format", "icon_palette", "name", "radii"]
        );
        assert_eq!(resolve(&from_value(&snapshot).unwrap()), resolve(&theme));
    }

    #[test]
    fn newer_requirements_keep_base_tables_without_reading_future_option_syntax() {
        let source = json!({"format":FORMAT,"name":"Future","base":"light","requires":2,"colors":{"panel":"#123456"},
            "radii":{"radius_sm":0},"options":{"future":"syntax"},"overlays":false});
        let theme = from_value(&source).unwrap();
        assert!(theme.needs_newer());
        assert!(theme.options.is_empty() && theme.overlays.is_empty() && ordered_options(&theme).is_empty());
        assert_eq!(resolve(&theme).panel, parse_color("#123456").unwrap());
        assert_eq!(resolve(&theme).radius_sm, 0);
        assert_eq!(from_value(&export_file(&theme)).unwrap(), theme);
        let restored: ThemeFile = serde_json::from_value(serde_json::to_value(&theme).unwrap()).unwrap();
        assert!(restored.needs_newer());
        assert_eq!(resolve(&restored), resolve_base(&theme));
        for invalid in [json!(-1), json!(1.5), json!("2"), Value::Null] {
            let mut source = source.clone();
            source["requires"] = invalid;
            assert!(from_value(&source).unwrap_err().contains("requires"));
        }
    }

    #[test]
    fn theme_options_opacity_only_changes_floating_fill_roles_and_full_is_exact() {
        for base in ["light", "dark"] {
            let mut source = json!({"format":FORMAT,"name":"Opacity","base":base,"options":[{"id":"opacity","type":"range"}]});
            let file = from_value(&source).unwrap();
            let original = resolve_base(&file);
            assert_eq!(resolve(&file), original, "omitted default is 100");
            assert_eq!(resolve_with(&file, &BTreeMap::from([("opacity".into(), OptionValue::Num(100.0))])), original);
            let faded = resolve_with(&file, &BTreeMap::from([("opacity".into(), OptionValue::Num(60.0))]));
            let mut only_fills = original;
            only_fills.panel = faded.panel;
            only_fills.panel_header = faded.panel_header;
            only_fills.dialog_bg = faded.dialog_bg;
            only_fills.dialog_body = faded.dialog_body;
            only_fills.dialog_footer = faded.dialog_footer;
            only_fills.overlay = faded.overlay;
            only_fills.nav_tip = faded.nav_tip;
            assert_eq!(faded, only_fills, "every other token (including text/icons/borders) stays exact");
            assert!(faded.panel.a() < original.panel.a(), "the effect must actually change a panel");
            source["options"] = json!([]);
            let without = from_value(&source).unwrap();
            assert_eq!(resolve_with(&without, &BTreeMap::from([("opacity".into(), OptionValue::Num(40.0))])), resolve_base(&without));
        }
    }

    #[test]
    fn theme_options_opacity_contrast_protects_text_at_40_over_viewport_extremes() {
        use crate::theme::{composite, contrast_ratio};
        for base in ["light", "dark"] {
            let file = from_value(&json!({"format":FORMAT,"name":"Contrast","base":base,
                "options":[{"id":"opacity","type":"range","default":40}]}))
            .unwrap();
            let full = resolve_base(&file);
            let faded = resolve(&file);
            for (role, fill) in [
                ("panel", faded.panel),
                ("panel_header", faded.panel_header),
                ("dialog_bg", faded.dialog_bg),
                ("dialog_body", faded.dialog_body),
                ("dialog_footer", faded.dialog_footer),
                ("overlay", faded.overlay),
            ] {
                for background in [Color32::BLACK, Color32::WHITE, full.viewport_top, full.viewport_bottom] {
                    let ground = composite(fill, background);
                    for ink in [faded.text, faded.form_ink, faded.dialog_label] {
                        assert!(contrast_ratio(composite(ink, ground), ground) >= 4.5, "{base} {role} {ink:?} on {background:?}: {fill:?}");
                    }
                }
            }
            for background in [Color32::BLACK, Color32::WHITE] {
                let ground = composite(faded.nav_tip, background);
                assert!(contrast_ratio(composite(faded.nav_tip_ink, ground), ground) >= 4.5);
            }
            assert!(faded.panel.a() > (f32::from(full.panel.a()) * 0.4).round() as u8, "raise the floor when requested alpha is unreadable");
            assert!(faded.panel.a() < 255, "do not make all options effectively opaque");
            assert_eq!(resolve_with(&file, &BTreeMap::from([("opacity".into(), OptionValue::Num(-10.0))])), faded, "theme API still clamps");
            assert_eq!(
                resolve_with(&file, &BTreeMap::from([("opacity".into(), OptionValue::Num(f32::NAN))])),
                faded,
                "non-finite values use defaults"
            );
        }
    }

    #[test]
    fn theme_options_opacity_is_applied_after_overlays_and_uses_the_theme_default() {
        let file = from_value(&json!({"format":FORMAT,"name":"Overlay opacity","base":"dark",
            "options":[{"id":"opacity","type":"range","default":60},{"id":"warm","type":"bool","default":true}],
            "overlays":[{"when":{"opt":"warm","eq":true},"colors":{"panel":"#111111"}}]}))
        .unwrap();
        let faded = resolve(&file);
        let full = resolve_with(&file, &BTreeMap::from([("opacity".into(), OptionValue::Num(100.0))]));
        assert_eq!(full.panel, Color32::from_rgb(17, 17, 17));
        assert!(faded.panel.a() < full.panel.a());
        assert_eq!(faded.text, full.text);
        // Premultiplied Color32 storage can quantize unmultiplied channels by one byte.
        assert!(faded.panel.to_srgba_unmultiplied()[..3].iter().all(|c| c.abs_diff(17) <= 1));
    }

    #[test]
    fn all_shipped_themes_keep_the_original_resolver_output_and_serialization() {
        assert_eq!(builtin().len(), BUILTIN.len());
        for theme in builtin() {
            let gruvbox = matches!(theme.id.as_str(), "gruvbox-dark" | "gruvbox-light");
            assert!(!theme.needs_newer());
            assert_eq!(!theme.options.is_empty(), gruvbox);
            assert_eq!(!theme.overlays.is_empty(), gruvbox);
            assert_eq!(resolve(theme), resolve_base(theme), "{}", theme.id);
            assert_eq!(resolve_with(theme, &BTreeMap::from([("opacity".into(), OptionValue::Num(40.0))])), resolve_base(theme), "{}", theme.id);
            let stored = serde_json::to_value(theme).unwrap();
            assert!(stored.get("requires").is_none(), "{}", theme.id);
            for key in ["options", "overlays"] {
                assert_eq!(stored.get(key).is_some(), gruvbox, "{}: {key}", theme.id);
            }
        }
    }

    #[test]
    fn malformed_saved_definitions_and_comparisons_resolve_without_panics() {
        let mut theme = from_value(&option_theme(
            sample_options(),
            json!([
                {"when":{"opt":"amount","gte":0},"colors":{"panel":"#ffffff"}}
            ]),
        ))
        .unwrap();
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let original = theme.clone();
            theme.options[2].kind = OptionKind::Range { default: invalid, min: 0.0, max: 10.0, step: 1.0, unit: None };
            assert_eq!(resolve(&theme), resolve_base(&theme));
            theme = original.clone();
            theme.options[2].kind = OptionKind::Range { default: 5.0, min: invalid, max: 10.0, step: 1.0, unit: None };
            assert_eq!(resolve(&theme), resolve_base(&theme));
            theme = original.clone();
            theme.overlays[0].when = Condition::Range { opt: "amount".into(), gte: Some(invalid), lte: None };
            assert_eq!(resolve(&theme), resolve_base(&theme));
            theme = original;
        }
        theme.requires = 2;
        assert_eq!(resolve(&theme), resolve_base(&theme));
        assert!(effective_values(&theme, &BTreeMap::new()).0.is_empty());
    }

    #[test]
    fn navigation_board_colours_are_themeable_and_roundtrip_on_both_bases() {
        for (base, tokens, colours) in [
            ("light", Tokens::light(), ["#e9edf2", "#dce2ea", "#8895a7", "#eef1f5", "#1b2430", "#ffffff", "#ffffff"]),
            ("dark", Tokens::dark(), ["#3a4150", "#464e60", "#7c879a", "#262b35", "#eef1f5", "#1b2430", "#2f343e"]),
        ] {
            for (actual, hex) in
                [tokens.nav_hover, tokens.nav_press, tokens.nav_dim, tokens.nav_viewport, tokens.nav_tip, tokens.nav_tip_ink, tokens.nav_field]
                    .into_iter()
                    .zip(colours)
            {
                assert_eq!(actual, parse_color(hex).unwrap(), "{base}: {hex}");
            }
            let old = from_value(&json!({"format":FORMAT,"name":"Old","base":base})).unwrap();
            assert_eq!(resolve(&old), tokens, "old themes retain the base's new navigation roles");
            assert_eq!(resolve(&from_value(&export("Board", old.base, &tokens)).unwrap()), tokens);
            let custom = from_value(&json!({"format":FORMAT,"name":"Custom","base":base,
                "colors":{"nav_press":"#123456","nav_tip":"#abcdef","nav_tip_key_border":"#01020380"}}))
            .unwrap();
            let custom_tokens = resolve(&custom);
            assert_eq!(custom_tokens.nav_press, parse_color("#123456").unwrap());
            assert_eq!(custom_tokens.nav_tip, parse_color("#abcdef").unwrap());
            assert_eq!(custom_tokens.nav_tip_key_border, parse_color("#01020380").unwrap());
            assert_eq!(resolve(&from_value(&export("Custom", custom.base, &custom_tokens)).unwrap()), custom_tokens);
        }
    }

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
        for id in ["gruvbox-dark", "gruvbox-light"] {
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
            let gruvbox = matches!(file.id.as_str(), "gruvbox-dark" | "gruvbox-light");
            let contrasts: &[&str] = if gruvbox { &["soft", "standard", "hard"] } else { &["standard"] };
            for contrast in contrasts {
                let options = if gruvbox { BTreeMap::from([("contrast".into(), OptionValue::Choice((*contrast).into()))]) } else { BTreeMap::new() };
                let t = resolve_with(file, &options);
                assert!(t.icon_palette.is_some(), "{} {contrast} has no icon palette", file.id);
                // A two-colour theme (1-bit, 1-bit Dark) can't give each workspace its own fill; it
                // still has to keep every tab readable, below.
                let two_colour = is_two_colour(&file.id);
                let mut fills = std::collections::HashSet::new();
                for tab in ["MODEL", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER", "SKETCH"] {
                    let accent = TabAccent::for_tab(&t, tab);
                    assert!(fills.insert(accent.color) || two_colour, "{} {contrast} repeats {tab}", file.id);
                    assert!(contrast_ratio(accent.ink, accent.soft) >= 4.5, "{} {contrast} {tab} soft text", file.id);
                    assert!(contrast_ratio(accent.on_fill(&t), accent.color) >= 4.5, "{} {contrast} {tab} pill text", file.id);
                }
            }
        }
    }

    #[test]
    fn one_bit_themes_are_complete_opaque_black_and_white() {
        for (id, name, base) in [("1-bit", "1-bit", Base::Light), ("1-bit-dark", "1-bit Dark", Base::Dark)] {
            assert!(is_two_colour(id));
            let file = find(id, &[]).unwrap();
            assert_eq!(file.name, name);
            assert_eq!(file.base, base);
            one_bit_is_complete_opaque_black_and_white(file);
        }
    }

    fn one_bit_is_complete_opaque_black_and_white(file: &ThemeFile) {
        let t = resolve(file);
        let exported = export(&file.name, file.base, &t);
        let mut resolved = t;
        for key in keys() {
            for color in slot(&mut resolved, key).unwrap() {
                let [r, g, b, _] = color.to_srgba_unmultiplied();
                assert!(matches!([r, g, b], [0, 0, 0] | [255, 255, 255]), "{}: {key}: {color:?}", file.id);
            }
            assert!(file.colors.contains_key(key), "{}: missing {key}", file.id);
        }
        fn check(value: &Value) {
            match value {
                Value::String(color) if color.starts_with('#') => {
                    assert!(matches!(color.as_str(), "#000000" | "#ffffff"), "{color}");
                }
                Value::Array(values) => values.iter().for_each(check),
                Value::Object(values) => values.values().for_each(check),
                _ => {}
            }
        }
        check(&exported);
        for role in crate::icon_svg::ROLE_NAMES.iter().filter(|role| **role != "gap") {
            assert!(file.icon_palette.contains_key(*role), "{}: missing icon role {role}", file.id);
        }
        for ramp in std::iter::once("solid").chain(crate::icon_svg::ACCENT_NAMES.iter().copied()) {
            assert!(file.icon_palette.contains_key(ramp), "{}: missing icon ramp {ramp}", file.id);
        }
        assert_eq!(radius_values(t), [0; 6]);
    }

    #[test]
    fn one_bit_selection_and_viewport_tokens_have_full_contrast() {
        for (id, paper, ink) in [("1-bit", Color32::WHITE, Color32::BLACK), ("1-bit-dark", Color32::BLACK, Color32::WHITE)] {
            one_bit_has_full_contrast(&resolve(find(id, &[]).unwrap()), id, paper, ink);
        }
    }

    /// `paper` is the theme's page (panels, viewport, bodies, hover); `ink` its text, and its selection.
    fn one_bit_has_full_contrast(t: &Tokens, id: &str, paper: Color32, ink: Color32) {
        assert_eq!((t.text, t.panel), (ink, paper), "{id}");
        for (fg, bg) in [
            (t.text, t.panel),
            (t.text_dim, t.field),
            (t.icon, t.toolbar),
            (t.app_bar_text, t.app_bar_tab_active),
            (t.selected_ink, t.selected_bg),
            (t.text, t.hover),
            (t.app_bar_text, t.app_bar_hover),
            (t.body_edge, t.body),
            (t.sel_edge, t.body_selected),
            (t.dimension, t.viewport_top),
            (t.sketch, t.viewport_bottom),
            (t.glyph_text, t.glyph_bg),
            (t.dialog_label, t.dialog_body),
        ] {
            assert_eq!(crate::theme::contrast_ratio(fg, bg), 21.0, "{id}: {fg:?} on {bg:?}");
        }
        assert_eq!((t.viewport_top, t.viewport_bottom, t.body), (paper, paper, paper), "{id}");
        assert_eq!((t.selected_bg, t.body_selected), (ink, ink), "{id}");
        // Hover fills are the paper, so a hovered row, card or tab never swallows its ink label.
        for hover in [t.hover, t.app_bar_hover, t.app_bar_hover_subtle, t.app_bar_hover_button, t.app_bar_hover_strong] {
            assert_eq!(hover, paper, "{id}: hover fill");
        }
        assert_eq!((t.body_edge, t.selected_ink), (ink, paper), "{id}");
        // Thumbnails and other pictures keep their own pixels: the tint stays neutral white.
        assert_eq!(t.file_image_tint, Color32::WHITE, "{id}");
    }

    #[test]
    fn gruvbox_contrast_choices_keep_six_appearances() {
        let families: Vec<_> = builtin().iter().filter(|f| f.id.starts_with("gruvbox-")).map(|f| f.id.as_str()).collect();
        assert_eq!(families, ["gruvbox-dark", "gruvbox-light"]);
        for (id, panels, tops, bottoms, inks) in [
            (
                "gruvbox-dark",
                ["#32302f", "#282828", "#1d2021"],
                ["#373432", "#32302f", "#2c2c2c"],
                ["#373432", "#32302f", "#2c2c2c"],
                ["#1d2021", "#1d2021", "#141617"],
            ),
            ("gruvbox-light", ["#f2e5bc", "#fbf1c7", "#f9f5d7"], ["#f6edd0", "#fcf5d8", "#fbf8e3"], ["#ebdbb2"; 3], ["#3c3836"; 3]),
        ] {
            let file = find(id, &[]).unwrap();
            assert_eq!(file.options.len(), 1);
            let option = &file.options[0];
            assert_eq!(option.id, "contrast");
            let OptionKind::Choice { default, values } = &option.kind else { panic!("contrast must be a choice") };
            assert_eq!(default, "standard");
            assert_eq!(values.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["soft", "standard", "hard"]);
            for (index, contrast) in ["soft", "standard", "hard"].into_iter().enumerate() {
                let options = BTreeMap::from([("contrast".into(), OptionValue::Choice(contrast.into()))]);
                let tokens = resolve_with(file, &options);
                assert_eq!(tokens.panel, parse_color(panels[index]).unwrap(), "{id}: {contrast}");
                assert_eq!(tokens.viewport_top, parse_color(tops[index]).unwrap(), "{id}: {contrast}");
                assert_eq!(tokens.viewport_bottom, parse_color(bottoms[index]).unwrap(), "{id}: {contrast}");
                assert_eq!(export_icon_palette(tokens.icon_palette)["ink"], inks[index], "{id}: {contrast}");
                assert_eq!(tokens.borders, crate::theme::Borders::LEGACY);
                assert_eq!(resolve_with(&from_value(&export_file(file)).unwrap(), &options), tokens);
            }
            assert_eq!(resolve(file), resolve_base(file), "the Standard default keeps the old family palette");
        }
    }

    #[test]
    fn gruvbox_retired_ids_stay_reserved_for_imported_names() {
        let mut custom = Vec::new();
        for (old, _, _) in GRUVBOX_ALIASES {
            let file = from_value(&json!({"format":FORMAT,"name":old,"base":"dark"})).unwrap();
            assert_eq!(add(&mut custom, file).unwrap(), format!("custom-{old}"));
            assert!(find(old, &custom).is_none(), "retired ids are not duplicate theme cards");
        }
    }

    #[test]
    fn builtin_order_preserves_the_existing_picker_sequence() {
        let original: &[&str] = &[
            "gruvbox-dark",
            "gruvbox-light",
            "solidcraft-95",
            "1-bit",
            "1-bit-dark",
            "platinum-9",
            "cupertino",
            "green-phosphor",
            "amber-phosphor",
            "blueprint",
            "one-dark",
            "tokyo-night",
            "solarized-light",
            "nord",
            "high-contrast",
            "graphite",
            "nosferatu",
            "catppuccin-mocha",
            "okibi",
            "akari",
            "clay",
            "clay-dark",
            "cga",
        ];
        let themes = read_builtin(BUILTIN).unwrap();
        let retained: Vec<_> = themes.iter().map(|t| t.id.as_str()).filter(|id| original.contains(id)).collect();
        assert_eq!(retained, original);
        assert!(BUILTIN.windows(2).all(|pair| pair[0].0 < pair[1].0), "source entries are sorted by id");
    }

    #[test]
    fn builtin_metadata_rejects_missing_invalid_and_duplicate_order() {
        let text =
            |order: Value| json!({"format": FORMAT, "name": "Synthetic", "base": "dark", "provenance": "Original", "order": order}).to_string();
        let good = text(json!(10));
        assert!(read_builtin(&[("a", &good)]).is_ok());
        let earlier = text(json!(5));
        let ordered = read_builtin(&[("a", &good), ("b", &earlier)]).unwrap();
        assert_eq!(ordered.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(read_builtin(&[("a", &good), ("a", &earlier)]).is_err());
        assert!(read_builtin(&[("a", &good), ("b", &good)]).is_err());
        for order in [Value::Null, json!(-1), json!(1.5), json!("10")] {
            assert!(read_builtin(&[("a", &text(order))]).is_err());
        }
        assert!(read_builtin(&[]).is_err());
    }

    #[test]
    fn built_in_themes_read_and_differ_from_their_base() {
        assert_eq!(builtin().len(), BUILTIN.len(), "every built-in theme file reads");
        for t in builtin() {
            let base = if t.base.is_dark() { Tokens::dark() } else { Tokens::light() };
            assert_ne!(resolve(t), base, "{}", t.id);
            if t.id.starts_with("gruvbox-") || is_two_colour(&t.id) {
                assert_eq!(t.base.is_dark(), t.id.contains("dark"), "{}", t.id);
            }
        }
    }

    /// CGA uses only the 16 colours of the IBM Color Graphics Adapter (alpha only on translucent
    /// tokens) and sets every colour, so nothing falls back to Dark's palette.
    #[test]
    fn cga_uses_only_the_sixteen_cga_colours() {
        const CGA: [&str; 16] = [
            "000000", "0000aa", "00aa00", "00aaaa", "aa0000", "aa00aa", "aa5500", "aaaaaa", "555555", "5555ff", "55ff55", "55ffff", "ff5555",
            "ff55ff", "ffff55", "ffffff",
        ];
        fn colours<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
            match v {
                Value::String(s) if s.starts_with('#') => out.push(s),
                Value::Array(a) => a.iter().for_each(|v| colours(v, out)),
                Value::Object(o) => o.values().for_each(|v| colours(v, out)),
                _ => {}
            }
        }
        let text = BUILTIN.iter().find(|(id, _)| *id == "cga").map(|(_, text)| *text).unwrap();
        let file: Value = serde_json::from_str(text).unwrap();
        let mut found = Vec::new();
        colours(&file, &mut found);
        assert!(found.len() > keys().len(), "CGA sets every colour and its icon palette");
        for c in found {
            let rgb = c.get(1..7).unwrap_or_default().to_ascii_lowercase();
            assert!(CGA.contains(&rgb.as_str()) && parse_color(c).is_some(), "{c} is not a CGA colour");
        }
        let theme = find("cga", &[]).unwrap();
        assert_eq!((theme.name.as_str(), theme.colors.len()), ("CGA", keys().len()), "CGA sets every colour");
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

    /// Okibi and Akari spend their ember on one or two marks a screen: the start page's rail is
    /// one of their neutrals, not their accent.
    #[test]
    fn okibi_and_akari_rails_are_neutral() {
        for id in ["okibi", "akari"] {
            let t = resolve(builtin().iter().find(|t| t.id == id).unwrap());
            for c in [t.file_rail, t.file_rail_hover] {
                let [r, g, b, _] = c.to_array();
                assert!(r.max(g).max(b) - r.min(g).min(b) <= 12, "{id}: {c:?} is tinted");
            }
        }
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

    /// Conflicting sketch geometry (#7244) is the error red by default, follows a theme's own
    /// `error` when the theme leaves `sketch_conflict` out, and takes the theme's own key when set.
    #[test]
    fn sketch_conflict_defaults_to_the_error_colour_and_a_theme_can_set_its_own() {
        for t in [Tokens::light(), Tokens::dark()] {
            assert_eq!(t.sketch_conflict, t.error);
        }
        let file = |colors: &str| {
            resolve(&parse(&format!(r#"{{"format":"solidcraft-theme/1","name":"Conflict","base":"light","colors":{{{colors}}}}}"#)).unwrap())
        };
        let (red, green) = (Color32::from_rgb(0xcc, 0x24, 0x1d), Color32::from_rgb(0x12, 0xab, 0x34));
        assert_eq!(file(r##""error":"#cc241d""##).sketch_conflict, red, "the theme's error colour");
        assert_eq!(file(r##""error":"#cc241d","sketch_conflict":"#12ab34""##).sketch_conflict, green, "the theme's own key wins");
        assert_eq!(file(r##""panel":"#102030""##).sketch_conflict, Tokens::light().error);
        for theme in builtin() {
            let t = resolve(theme);
            assert_eq!(t.sketch_conflict, t.error, "{}: conflicts in the theme's error colour", theme.id);
        }
        assert!(keys().contains(&"sketch_conflict"), "exported and imported by name");
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
        for id in ["gruvbox-dark", "gruvbox-light"] {
            for contrast in ["soft", "standard", "hard"] {
                let options = BTreeMap::from([("contrast".into(), OptionValue::Choice(contrast.into()))]);
                assert_eq!(radius_values(resolve_with(find(id, &[]).unwrap(), &options)), defaults);
            }
        }
        for file in builtin().iter().filter(|file| file.radii.is_empty()) {
            assert_eq!(radius_values(resolve(file)), defaults, "{}", file.id);
        }
    }

    #[test]
    fn a_followed_file_reloads_when_rewritten_and_keeps_the_last_good_theme() {
        let dir = std::env::temp_dir().join(format!("solidcraft-theme-follow-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("live.json");
        let path = file.to_string_lossy().to_string();
        let write = |panel: &str, extra: &str| {
            std::fs::write(&file, format!(r##"{{"format":"{FORMAT}","name":"Live","base":"dark","colors":{{"panel":"{panel}"{extra}}}}}"##)).unwrap();
        };
        let panel = |f: &Follow| f.theme().map(|t| resolve(t).panel);

        // Off by default: nothing is followed or read.
        let mut follow = Follow::default();
        assert_eq!(follow.path(""), "");
        assert_eq!(follow.poll("", 0.0, true), FollowChange::None);
        assert!(follow.theme().is_none() && follow.error().is_none());

        // A valid file applies; an unchanged one is not read again.
        write("#102030", "");
        assert_eq!(follow.poll(&path, 0.0, false), FollowChange::Loaded("Live".into()));
        assert_eq!(panel(&follow), Some(Color32::from_rgb(0x10, 0x20, 0x30)));
        assert_eq!(follow.poll(&path, 0.5, false), FollowChange::None, "polled at most every second");
        assert_eq!(follow.poll(&path, 1.5, false), FollowChange::None, "unchanged");

        // Rewritten (another size, so even a coarse modified time sees it): reloaded.
        write("#405060", r##","accent":"#aa3300""##);
        assert_eq!(follow.poll(&path, 1.6, false), FollowChange::None, "not due yet");
        assert_eq!(follow.poll(&path, 2.6, false), FollowChange::Loaded("Live".into()));
        assert_eq!(panel(&follow), Some(Color32::from_rgb(0x40, 0x50, 0x60)));

        // Half written, broken, missing or too big: the last good theme stays, the reason once.
        std::fs::write(&file, r#"{"format":"solidcraft-theme/1","name":"Li"#).unwrap();
        assert!(matches!(follow.poll(&path, 4.0, false), FollowChange::Failed(e) if e.contains("not a theme file")));
        assert_eq!(follow.poll(&path, 5.0, true), FollowChange::None, "the same reason is reported once");
        assert_eq!(panel(&follow), Some(Color32::from_rgb(0x40, 0x50, 0x60)));
        std::fs::write(&file, vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(matches!(follow.poll(&path, 6.0, false), FollowChange::Failed(e) if e.contains("KiB")));
        std::fs::remove_file(&file).unwrap();
        assert!(matches!(follow.poll(&path, 7.0, false), FollowChange::Failed(e) if e.contains("can't read")));
        assert_eq!(panel(&follow), Some(Color32::from_rgb(0x40, 0x50, 0x60)));
        assert!(follow.error().is_some());
        assert!(matches!(follow.poll(&dir.to_string_lossy(), 8.0, false), FollowChange::Failed(e) if e.contains("not a file")));

        // Written again completely: back on, the error gone.
        write("#708090", "");
        assert_eq!(follow.poll(&path, 9.0, false), FollowChange::Loaded("Live".into()));
        assert_eq!((panel(&follow), follow.error()), (Some(Color32::from_rgb(0x70, 0x80, 0x90)), None));

        // The launch flag's path wins over the preference's; an empty path stops following.
        follow.launch = Some(path.clone());
        assert_eq!(follow.path("/elsewhere.json"), path);
        follow.launch = None;
        assert_eq!(follow.poll("", 10.0, false), FollowChange::None);
        assert!(follow.theme().is_none());
        assert!(follow_path(&"x".repeat(MAX_FOLLOW_PATH + 1)).is_err() && follow_path("a\nb").is_err());
        assert!(follow_path("a\u{7}b").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
