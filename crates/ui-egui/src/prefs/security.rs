//! Security preferences: signing saved designs and checking signatures when they open.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_page(ui, |ui| {
        let t = Tokens::get();
        let labels = t.space_24 * 6.0 + t.space_6;
        ui_kit::settings_heading(ui, app.tr("prefs.security"), app.tr("prefs.security_note"));
        ui_kit::settings_section(ui, "signing", app.tr("prefs.security.signing"), |ui| {
            setting::sign_saved_designs(app, ui);
            ui_kit::settings_form(ui, labels, |ui| {
                let tip = app.tr("prefs.security.signing_key_help").to_string();
                ui_kit::settings_row_help(ui, app.tr("prefs.security.signing_key"), &tip, |ui| {
                    let hint = app.tr("prefs.security.signing_key_hint").to_string();
                    let (_, response) = ui_kit::text_field_reserving(ui, &mut app.preferences.signing_key, &hint, 0.0);
                    crate::scenario::publish_target("prefs:signing_key", response.rect);
                    response
                });
            });
            setting::verify_opened_designs(app, ui);
        });
        ui_kit::settings_section(ui, "encryption", app.tr("prefs.security.encryption"), |ui| {
            ui_kit::settings_inline_note(ui, app.tr("prefs.security.encryption_note"));
        });
    });
}
