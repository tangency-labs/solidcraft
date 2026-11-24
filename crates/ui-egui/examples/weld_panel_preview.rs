//! Run with `cargo run -p solidcraft-ui-egui --example weld_panel_preview`.
//! A UI-only specimen: actions are shown locally and never sent to an engine.
use solidcraft_theme::Tokens;
use solidcraft_ui_egui::{
    ui_kit,
    weld_panel::{self, WeldNotice, WeldPanelEvent, WeldPanelState, WeldType},
};

struct Preview {
    dark: bool,
    cards: [WeldPanelState; 4],
    last: WeldPanelEvent,
}

impl Preview {
    fn new(ctx: &egui::Context) -> Self {
        solidcraft_look::install(ctx);
        let mut advanced = WeldPanelState::specimen(WeldType::Fillet, true);
        advanced.notices = WeldNotice::all_samples();
        Self {
            dark: std::env::var_os("WELD_PREVIEW_DARK").is_some(),
            cards: [
                WeldPanelState::specimen(WeldType::Fillet, false),
                WeldPanelState::specimen(WeldType::Groove, false),
                WeldPanelState::specimen(WeldType::Spot, false),
                advanced,
            ],
            last: WeldPanelEvent::None,
        }
    }
}

impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        solidcraft_theme::set_dark(self.dark);
        solidcraft_theme::apply(ui.ctx());
        let t = Tokens::get();
        ui.painter().rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
        ui.add_space(t.space_12);
        ui.horizontal(|ui| {
            ui_kit::checkbox(ui, solidcraft_ui_text::tr("ui.weld.preview.dark"), &mut self.dark);
            ui_kit::text(ui, &solidcraft_ui_text::format("ui.weld.preview.event", &[("event", &format!("{:?}", self.last))]), ui_kit::Text::Inline);
        });
        let card_height = (ui.available_height() - t.space_12).max(0.0);
        let area = egui::ScrollArea::horizontal().auto_shrink([false, false]);
        area.show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = t.space_16;
            ui.horizontal_top(|ui| {
                for (index, state) in self.cards.iter_mut().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.set_max_height(card_height);
                        let event = weld_panel::show(ui, state, &t);
                        if event != WeldPanelEvent::None {
                            self.last = event;
                        }
                    });
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    solidcraft_ui_egui::i18n::install();
    let title = solidcraft_ui_text::tr("ui.weld.preview.title");
    eframe::run_native(
        title,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1480.0, 960.0]).with_resizable(false),
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(Preview::new(&cc.egui_ctx)))),
    )
}
