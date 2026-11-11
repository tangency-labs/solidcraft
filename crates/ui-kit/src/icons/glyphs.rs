//! Code-drawn glyphs: small control marks the kit and its components draw (transport, window
//! caption, badges, pin, cross, breadcrumb, view compass tips, the agent pointer, the brand mark).
//!
//! Like the icons, they are the host's artwork: the host fills a `static` [`Glyphs`] with its
//! painters and [`install`]s it once, before the first frame and in its test contexts. Before
//! that every glyph draws nothing and [`axis_radius`] is 0.
use std::sync::OnceLock;

use egui::{Color32, Painter, Pos2, Rect, Stroke, Ui};
use solidcraft_theme::Tokens;

use crate::window_chrome::Caption;

/// The host's glyph painters.
#[derive(Clone, Copy)]
pub struct Glyphs {
    /// A history transport mark centred on the point: `first`, `back`, `forward`, `play`, `pause`
    /// or (any other name) last.
    pub transport: fn(&Painter, Pos2, &str, Color32),
    /// A window caption button's mark centred on the point; `bool` is whether the window is maximised.
    pub caption: fn(&Ui, Caption, bool, Pos2, Stroke),
    /// A small status badge (`lock`, …) in the rectangle.
    pub badge: fn(&Painter, Rect, &str, Color32),
    /// A pin in the rectangle; `bool` is whether it is pinned.
    pub pin: fn(&Painter, Rect, bool, Color32),
    /// A cross filling the rectangle.
    pub cross: fn(&Painter, Rect, Stroke),
    /// A path separator centred on the point.
    pub breadcrumb: fn(&Painter, Pos2, Color32),
    /// A view compass axis end: compass centre, axis name, direction, tip position, depth,
    /// hovered, tokens and uniform scale.
    pub axis_tip: fn(&Painter, Pos2, &str, [f64; 3], Pos2, f64, bool, Tokens, f32),
    /// The pointer target radius around a compass end, in points.
    pub axis_radius: f32,
    /// The agent pointer with its tip at the point.
    pub pointer: fn(&Painter, Pos2),
    /// The host's brand mark in one ink: centre, height.
    pub brand_mark: fn(&Painter, Pos2, f32, Color32),
}

static INSTALLED: OnceLock<&'static Glyphs> = OnceLock::new();

/// Install the host's glyphs, once per process. Installing the same glyphs again does nothing; a
/// different set is ignored and logged.
pub fn install(glyphs: &'static Glyphs) {
    if let Err(ignored) = INSTALLED.set(glyphs)
        && let Some(installed) = INSTALLED.get()
        && !std::ptr::eq(*installed, ignored)
    {
        log::warn!("glyphs are already installed: a second, different set is ignored");
    }
}

fn installed() -> Option<&'static Glyphs> {
    INSTALLED.get().copied()
}

/// [`Glyphs::transport`].
pub fn transport_glyph(p: &Painter, c: Pos2, what: &str, col: Color32) {
    if let Some(g) = installed() {
        (g.transport)(p, c, what, col);
    }
}

/// [`Glyphs::caption`].
pub fn paint_glyph(ui: &Ui, c: Caption, maximized: bool, at: Pos2, s: Stroke) {
    if let Some(g) = installed() {
        (g.caption)(ui, c, maximized, at, s);
    }
}

/// [`Glyphs::badge`].
pub fn badge(p: &Painter, r: Rect, name: &str, c: Color32) {
    if let Some(g) = installed() {
        (g.badge)(p, r, name, c);
    }
}

/// [`Glyphs::pin`].
pub fn pin_icon(p: &Painter, r: Rect, on: bool, col: Color32) {
    if let Some(g) = installed() {
        (g.pin)(p, r, on, col);
    }
}

/// [`Glyphs::cross`].
pub fn cross(p: &Painter, r: Rect, s: Stroke) {
    if let Some(g) = installed() {
        (g.cross)(p, r, s);
    }
}

/// [`Glyphs::breadcrumb`].
pub fn breadcrumb(p: &Painter, c: Pos2, col: Color32) {
    if let Some(g) = installed() {
        (g.breadcrumb)(p, c, col);
    }
}

/// [`Glyphs::axis_tip`] at scale 1.
pub fn axis_tip(painter: &Painter, c: Pos2, name: &str, d: [f64; 3], at: Pos2, depth: f64, hovered: bool, t: Tokens) {
    axis_tip_scaled(painter, c, name, d, at, depth, hovered, t, 1.0);
}

/// [`Glyphs::axis_tip`].
pub fn axis_tip_scaled(painter: &Painter, c: Pos2, name: &str, d: [f64; 3], at: Pos2, depth: f64, hovered: bool, t: Tokens, scale: f32) {
    if let Some(g) = installed() {
        (g.axis_tip)(painter, c, name, d, at, depth, hovered, t, scale);
    }
}

/// [`Glyphs::axis_radius`], or 0 before the glyphs are installed.
pub fn axis_radius() -> f32 {
    installed().map_or(0.0, |g| g.axis_radius)
}

/// [`Glyphs::pointer`].
pub fn pointer(painter: &Painter, p: Pos2) {
    if let Some(g) = installed() {
        (g.pointer)(painter, p);
    }
}

/// [`Glyphs::brand_mark`].
pub fn brand_mark_silhouette(p: &Painter, c: Pos2, height: f32, ink: Color32) {
    if let Some(g) = installed() {
        (g.brand_mark)(p, c, height, ink);
    }
}
