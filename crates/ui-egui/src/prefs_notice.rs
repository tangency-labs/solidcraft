//! The notice for settings put back at start because they would keep SolidCraft from starting
//! on this computer (`SolveApp::settings_reset`), shown until dismissed.

use crate::SolveApp;
use crate::ui_kit;

/// Draw the notice while any setting was reset. OK dismisses it; "Open Preferences" dismisses it
/// and opens the Graphics page.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.settings_reset.is_empty() {
        return;
    }
    let (closed, action) = ui_kit::Window::new(egui::Id::unique("sc_settings_reset"), "settings", app.tr("settings.reset_title"))
        .actions(app.tr("ui.open_preferences"), app.tr("ui.heading_ok"), None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, app.tr("settings.reset_intro"), ui_kit::Text::Report("intro", false));
            for (index, reset) in app.settings_reset.iter().enumerate() {
                ui_kit::text(ui, &format!("• {}", reset.message()), ui_kit::Text::Report(&format!("reset:{index}"), false));
            }
        });
    if closed || action != ui_kit::Action::None {
        app.settings_reset.clear();
    }
    if action == ui_kit::Action::Ok {
        crate::prefs::open_at(app, crate::prefs::PAGE_GRAPHICS);
    }
}
