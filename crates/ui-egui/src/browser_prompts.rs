//! The prompt the Browser opens over the viewport: the Capture Position question asked before a
//! command or a save while components are moved but not captured. (A component moves in
//! Move/Copy, `dialogs_move`.)

use crate::SolveApp;
use crate::browser::answer_capture;
use crate::ui_kit;

/// The Capture Position question: capture, revert or cancel the waiting command (or save).
pub(crate) fn capture_prompt(app: &mut SolveApp, ctx: &egui::Context) {
    if app.tree.capture_prompt.is_none() {
        return;
    }
    let n = app.session.uncaptured_position_count();
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_capture_prompt"), "capture", solidcraft_ui_text::tr_label("Capture Position"))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, -80.0))
        .actions("Capture Position", "Cancel", Some("Revert"))
        .show(ctx, true, |ui| {
            let question = if n == 1 {
                solidcraft_ui_text::tr("browser.capture_one").to_string()
            } else {
                solidcraft_ui_text::format("browser.capture_many", &[("count", &n.to_string())])
            };
            ui_kit::text(ui, &question, ui_kit::Text::Report("question", false));
            ui_kit::text(ui, solidcraft_ui_text::tr("browser.capture_note"), ui_kit::Text::Note("capture"));
        });
    let answer = match action {
        ui_kit::Action::Ok => Some("capture"),
        ui_kit::Action::Apply => Some("revert"),
        ui_kit::Action::Cancel => Some("cancel"),
        ui_kit::Action::None if closed => Some("cancel"),
        ui_kit::Action::None => None,
    };
    if let Some(answer) = answer {
        let _ = answer_capture(app, answer);
    }
}
