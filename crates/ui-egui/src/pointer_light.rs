//! A light that follows the pointer (Preferences › Appearance › Light follows the pointer), for
//! any part of the interface or of a theme that should move with it: background patterns drift,
//! a glass theme's streaks of light slide, and a future theme's own effects can do the same by
//! reading [`light`] or [`drift`]. Off by default; a theme can turn it on through its layout.
//!
//! `prefs::apply` calls [`update`] every frame. While it is on, the light eases after the pointer
//! and settles in the middle when the pointer leaves the window; while it is off, [`light`] is
//! zero, so nothing that reads it moves and nothing asks for frames.

use std::cell::Cell;

use egui::Vec2;

/// How long the light takes to catch up with the pointer, in seconds.
const EASE_SECONDS: f32 = 1.2;
/// How far a layer at depth 1 drifts, as a share of the area it is drawn in, at the light's
/// furthest across and down.
const DRIFT: Vec2 = egui::vec2(0.03, 0.02);

thread_local! {
    /// Where the light comes from: the pointer's place from the window's middle, -1 to 1 each way,
    /// eased; zero while it doesn't follow the pointer.
    static LIGHT: Cell<Vec2> = const { Cell::new(Vec2::ZERO) };
}

/// Follow the pointer or not (`prefs::apply`, every frame). While `on`, the light eases toward the
/// pointer's place in the window, and to the middle when the pointer is away.
pub fn update(ctx: &egui::Context, on: bool) {
    let light = if on {
        let screen = ctx.content_rect();
        let target = ctx
            .input(|i| i.pointer.latest_pos())
            .filter(|_| screen.width() > 0.0 && screen.height() > 0.0)
            .map_or(Vec2::ZERO, |p| (p - screen.center()) / (screen.size() * 0.5));
        crate::motion::slide(ctx, egui::Id::unique("sc_pointer_light"), target, EASE_SECONDS)
    } else {
        Vec2::ZERO
    };
    set(light);
}

/// Use `offset` as the light's place (-1 to 1 each way from the middle); not a number: the middle.
fn set(offset: Vec2) {
    LIGHT.set(if offset.is_finite() { offset.clamp(Vec2::splat(-1.0), Vec2::splat(1.0)) } else { Vec2::ZERO });
}

/// Where the light comes from now: -1 to 1 each way from the window's middle; zero while it doesn't
/// follow the pointer.
pub fn light() -> Vec2 {
    LIGHT.get()
}

/// How far a layer `depth` deep (1: the usual, more: nearer, so it moves more) drifts with the
/// light in an area of `size`. Zero while the light doesn't follow the pointer.
pub fn drift(size: Vec2, depth: f32) -> Vec2 {
    let depth = if depth.is_finite() { depth.clamp(0.0, 4.0) } else { 0.0 };
    light() * size * DRIFT * depth
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_stays_in_the_middle_while_off_and_within_the_window_while_on() {
        let ctx = egui::Context::default();
        update(&ctx, false);
        assert_eq!(light(), Vec2::ZERO);
        assert_eq!(drift(egui::vec2(800.0, 600.0), 1.0), Vec2::ZERO, "nothing moves while it is off");
        set(egui::vec2(5.0, -9.0));
        assert_eq!(light(), egui::vec2(1.0, -1.0));
        assert_eq!(drift(egui::vec2(100.0, 100.0), 1.0), egui::vec2(3.0, -2.0));
        assert_eq!(drift(egui::vec2(100.0, 100.0), f32::NAN), Vec2::ZERO);
        set(egui::vec2(f32::NAN, 0.0));
        assert_eq!(light(), Vec2::ZERO);
    }
}
