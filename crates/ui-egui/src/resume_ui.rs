//! Recover the egui chrome after the display wakes from sleep (#8749).
//!
//! On Linux (Wayland + NVIDIA especially) the window can stay black after wake: the device is not
//! reported lost, so SolidCraft does not restart, but GPU-backed egui textures (font atlas, icons)
//! are stale. The viewport often comes back on scroll or a tab change because it rebuilds its own
//! meshes; the ribbon and panels stay blank until those textures are uploaded again.
//!
//! When egui's frame clock jumps by more than [`LONG_PAUSE`], treat that as a wake and refresh.

use std::sync::Arc;

use egui::{ImageData, TextureId, TextureOptions};

/// A gap this long between frames means the process was not painting (sleep, lock screen, …).
pub const LONG_PAUSE: f64 = 1.5;

/// Re-upload the font atlas, drop cached icon GPU textures, and ask for another frame.
pub fn after_long_pause(ctx: &egui::Context) {
    refresh_font_atlas(ctx);
    crate::icons::clear_texture_cache(ctx);
    ctx.forget_all_images();
    ctx.request_repaint();
}

/// Push the full font atlas so the backend replaces TextureId::default on the GPU.
fn refresh_font_atlas(ctx: &egui::Context) {
    let image = ctx.fonts(|fonts| fonts.image());
    let delta = egui::epaint::ImageDelta::full(ImageData::Color(Arc::new(image)), TextureOptions::default());
    ctx.tex_manager().write().set(TextureId::default(), delta);
}

/// True when `now` is more than [`LONG_PAUSE`] after `previous` (and both are finite and ordered).
pub fn is_long_pause(previous: f64, now: f64) -> bool {
    previous.is_finite() && now.is_finite() && previous > 0.0 && now > previous && (now - previous) > LONG_PAUSE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_pause_is_a_jump_in_frame_time() {
        assert!(!is_long_pause(0.0, 10.0), "first frame has no previous");
        assert!(!is_long_pause(1.0, 1.2));
        assert!(is_long_pause(1.0, 1.0 + LONG_PAUSE + 0.01));
        assert!(!is_long_pause(f64::NAN, 10.0));
    }

    #[test]
    fn after_long_pause_queues_a_full_font_atlas_upload() {
        let ctx = egui::Context::default();
        crate::fonts::setup_bundled_fonts(&ctx);
        // Build the atlas once so TextureId::default exists.
        let mut primed = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("wake");
        });
        primed.textures_delta.clear();
        after_long_pause(&ctx);
        // The refresh is applied as a tex_manager delta taken at the start of the next pass.
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("again");
        });
        let font = out.textures_delta.set.get(&TextureId::default());
        assert!(
            font.is_some_and(|deltas| deltas.iter().any(|d| d.pos.is_none() && matches!(&d.image, ImageData::Color(img) if !img.pixels.is_empty()))),
            "full font atlas delta with pixels"
        );
        out.textures_delta.clear();
    }
}
