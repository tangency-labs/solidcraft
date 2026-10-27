//! Native W36 interaction harness: cargo run --profile ci -p solidcraft --example broken_view
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use solidcraft_ui_egui::broken_view_preview::BreakPreview;
use solidcraft_ui_egui::ui_kit::{Container, Surface};

#[derive(Default)]
struct App {
    preview: BreakPreview,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let tokens = solidcraft_ui_egui::theme::Tokens::get();
        Container::new(&ctx, Surface::Viewport)
            .frame(&ctx, tokens.panel, solidcraft_ui_egui::frame::Side::Right)
            .inner_margin(tokens.space_12)
            .show(ui, |ui| self.preview.show(ui));
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        visuals.window_fill().to_normalized_gamma_f32()
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "SolidCraft — Broken View interaction preview",
        eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 820.0]), ..Default::default() },
        Box::new(|cc| {
            solidcraft_ui_egui::theme::apply(&cc.egui_ctx);
            Ok(Box::<App>::default())
        }),
    )
}
