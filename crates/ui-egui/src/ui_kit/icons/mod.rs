//! Icons: the kit draws them; the host supplies them.
//!
//! The kit ships no artwork. A host keeps its icons in a `static` [`IconSet`] and [`install`]s it
//! once, before the first frame (and in every test context that draws icons). Every painter here
//! draws from that set, in the set's colour roles for the current theme, accent and colour vision;
//! colours never come from arbitrary caller tints. A name the set does not know draws the set's
//! placeholder master; with no set installed nothing is drawn.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use egui::{Color32, Painter, Rect, TextureHandle, TextureOptions, pos2};

pub mod glyphs;
mod svg;

pub use svg::{
    Doc, IconData, IconSet, Look, MAX_ACCENTS, MAX_ROLES, PLACEHOLDER, Palette, Resolved, audit, render, render_tinted, set_theme_palette,
    theme_palette, theme_palette_state,
};

static INSTALLED: OnceLock<&'static IconSet> = OnceLock::new();

/// No artwork: what the painters draw from before a set is installed.
static NO_ICONS: IconSet = IconSet::new(IconData::EMPTY);

/// Install the host's icons, once per process, before the first frame. Installing the same set
/// again does nothing; a different set is ignored and logged.
pub fn install(set: &'static IconSet) {
    if let Err(ignored) = INSTALLED.set(set)
        && let Some(installed) = INSTALLED.get()
        && !std::ptr::eq(*installed, ignored)
    {
        log::warn!("an icon set is already installed: a second, different set is ignored");
    }
}

/// The installed set, or an empty one before [`install`].
pub fn installed() -> &'static IconSet {
    INSTALLED.get().copied().unwrap_or(&NO_ICONS)
}

/// [`IconSet::resolve`] in the installed set.
pub fn resolve(name: &str) -> Resolved {
    installed().resolve(name)
}

/// [`IconSet::doc_for`] in the installed set.
pub fn doc_for(master: &str, px: u32) -> Option<&'static Doc> {
    installed().doc_for(master, px)
}

/// [`IconSet::accent_index`] in the installed set.
pub fn accent_index(name: &str) -> Option<usize> {
    installed().accent_index(name)
}

/// [`IconSet::primary`] in the installed set.
pub fn primary(name: &str, dark: bool, vision: solidcraft_theme::ColorVision) -> Color32 {
    installed().primary(name, dark, vision)
}

/// [`IconSet::role_colour`] in the installed set.
pub fn role_colour(name: &str, look: Look) -> Color32 {
    installed().role_colour(name, look)
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Finish {
    Artwork,
    Ink(Color32),
    Danger,
}

/// Textures already rasterised: one per (master, look, pixel size, finish).
#[derive(Clone, Default)]
struct Cache(Arc<Mutex<CacheEntries>>);

/// Drop rasterised icon textures so the next paint uploads them again (wake from sleep, #8749).
pub fn clear_texture_cache(ctx: &egui::Context) {
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::unique("sc_icon_cache"), Cache::default).clone());
    if let Ok(mut entries) = cache.0.lock() {
        entries.textures.clear();
    }
}

#[derive(Default)]
struct CacheEntries {
    palette: Option<Palette>,
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
pub fn paint_app_bar(p: &Painter, r: Rect, name: &str, opacity: f32) {
    paint_with_ink(p, r, name, Some(true), opacity, Some(solidcraft_theme::Tokens::get().app_bar_text), Mirror::None);
}

/// Paint a single-colour icon in `ink`.
pub fn paint_ink(p: &Painter, r: Rect, name: &str, ink: Color32) {
    paint_with_ink(p, r, name, Some(true), 1.0, Some(ink), Mirror::None);
}

/// Paint a single-colour icon in `ink`, mirrored top to bottom (a chevron pointing up).
pub fn paint_ink_flipped(p: &Painter, r: Rect, name: &str, ink: Color32) {
    paint_with_ink(p, r, name, Some(true), 1.0, Some(ink), Mirror::Vertical);
}

/// Paint a single-colour icon mirrored left to right, reusing the original master.
pub fn paint_ink_mirrored(p: &Painter, r: Rect, name: &str, ink: Color32) {
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
    let resolved = resolve(name);
    let look = Look::themed(dark.unwrap_or_else(|| ctx.global_style().visuals.dark_mode), resolved.accent, solidcraft_theme::vision());
    paint_resolved(p, r, resolved, look, opacity, ink.map_or(Finish::Artwork, Finish::Ink), mirror);
}

/// Draw with a named palette ramp, preserving outlines, passive faces and transparency.
/// `None` explicitly selects base blue; an unknown name retains the icon's own accent.
/// Existing painters continue to select the icon's semantic accent.
pub fn paint_accented(p: &Painter, r: Rect, name: &str, accent: Option<&str>, opacity: f32) {
    let resolved = resolve(name);
    let accent = match accent {
        Some(name) => accent_index(name).or(resolved.accent),
        None => None,
    };
    let look = Look::themed(p.ctx().global_style().visuals.dark_mode, accent, solidcraft_theme::vision());
    paint_resolved(p, r, resolved, look, opacity, Finish::Artwork, Mirror::None);
}

/// Shared destructive-action artwork: shaded faces in the set's danger ramp, normal outlines and cutouts.
/// Unlike a single-colour mask, this also tints a master's passive faces through
/// that ramp without flattening the lid, handle or other details.
pub fn paint_danger(p: &Painter, r: Rect, name: &str, opacity: f32) {
    let set = installed();
    let resolved = set.resolve(name);
    let look = Look::themed(p.ctx().global_style().visuals.dark_mode, set.accent_index(set.data().danger_accent), solidcraft_theme::vision());
    paint_resolved(p, r, resolved, look, opacity, Finish::Danger, Mirror::None);
}

fn paint_resolved(p: &Painter, r: Rect, resolved: Resolved, look: Look, opacity: f32, finish: Finish, mirror: Mirror) {
    let ctx = p.ctx();
    let ppp = ctx.pixels_per_point().max(0.5);
    let side = r.width().min(r.height());
    let px = (side * ppp).round().clamp(8.0, 512.0) as u32;
    let cache = ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::unique("sc_icon_cache"), Cache::default).clone());
    let Ok(mut entries) = cache.0.lock() else { return };
    let palette = theme_palette(solidcraft_theme::is_dark());
    if entries.palette != palette {
        entries.textures.clear();
        entries.palette = palette;
    }
    let map = &mut entries.textures;
    let key = (resolved.master, look, px, finish);
    if let std::collections::hash_map::Entry::Vacant(slot) = map.entry(key) {
        let Some(doc) = doc_for(resolved.master, px) else { return };
        let pm = if finish == Finish::Danger { render_tinted(doc, look, px) } else { render(doc, look, px) };
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
