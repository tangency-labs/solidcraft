//! Interaction preferences: how the pointer and the keys act on the model (#4128).
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_heading(ui, app.tr("prefs.structure.interaction"), app.tr("prefs.structure.interaction_note"));
    ui_kit::settings_section(ui, "hide_under_cursor", crate::i18n::tr_label("Show/Hide"), |ui| {
        let mut hide = app.session.hide_under_cursor;
        let check = ui_kit::checkbox(ui, app.tr("prefs.hide_under_cursor"), &mut hide).on_hover_text(app.tr("prefs.hide_under_cursor_note"));
        crate::scenario::publish_target("prefs:hide_under_cursor", check.rect);
        if hide != app.session.hide_under_cursor {
            app.session.hide_under_cursor = hide;
        }
    });
}
