//! Review evidence from the actual developer UI, without taking over the desktop.
#[allow(dead_code)]
#[path = "support/raster.rs"]
mod raster;

use solidcraft_ui_egui::{
    broken_view_preview::BreakPreview,
    scenario::Harness,
    theme,
    ui_kit::{Container, Surface},
};
use std::collections::HashMap;

#[test]
fn render_oblique_sample_for_alignment_review() {
    let mut h = Harness::new();
    h.ctx = egui::Context::default();
    solidcraft_ui_egui::fonts::setup_fonts(&h.ctx);
    theme::set_dark(true);
    theme::apply(&h.ctx);
    let mut preview = BreakPreview::oblique_sample();
    let mut textures = HashMap::new();
    let mut output = None;
    for frame in 0..8 {
        let mut out = h.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 820.0))),
                time: Some(f64::from(frame) / 15.0),
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                let tokens = theme::Tokens::get();
                Container::new(&ctx, Surface::Viewport)
                    .frame(&ctx, tokens.panel, solidcraft_ui_egui::frame::Side::Right)
                    .inner_margin(tokens.space_12)
                    .show(ui, |ui| preview.show(ui));
            },
        );
        raster::collect_textures(&mut textures, &mut out);
        output = Some(out);
    }
    let screenshot = raster::raster(&mut h, &textures, output.unwrap());
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/broken-view-review");
    std::fs::create_dir_all(&dir).unwrap();
    screenshot.save(dir.join("aligned-oblique.png")).unwrap();
}
