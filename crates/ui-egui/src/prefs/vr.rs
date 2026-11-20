//! The XR preferences page (docs/vr.md), for now VR only: whether the app starts in VR, and
//! what this run is doing about it. Few people have a headset, so XR keeps its settings on this
//! page of its own, in the Input group after the 3D mouse; the settings VR gains, and AR or other
//! XR later, go here too.
use super::*;

/// Connect (a restart into VR, as VR is chosen when the window opens) or, while a session is open,
/// Disconnect (it ends; the window goes on). Connect is offered only where the app can restart
/// itself.
fn connect_row(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.services.vr_connected {
        let response = ui_kit::button(ui, app.tr("prefs.vr_page.disconnect"), ui_kit::Button::Normal)
            .backing_on_hover_text(app.tr("prefs.vr_page.disconnect_help"));
        crate::scenario::publish_target("prefs:vr_disconnect", response.rect);
        if response.clicked() {
            app.services.vr_disconnect = true;
        }
    } else if app.services.graphics_restart.is_some() {
        let response =
            ui_kit::button(ui, app.tr("prefs.vr_page.connect"), ui_kit::Button::Normal).backing_on_hover_text(app.tr("prefs.vr_page.connect_help"));
        crate::scenario::publish_target("prefs:vr_connect", response.rect);
        if response.clicked() {
            crate::update::restart_into_vr(app);
        }
    }
}

/// The XR page. Where the host has no VR (macOS, the browser), it only says what VR needs.
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.structure.vr"), app.tr("prefs.structure.vr_note"));
        let Some(status) = app.services.vr_status.clone() else {
            ui_kit::settings_inline_note(ui, app.tr("prefs.vr_page.unavailable"));
            return;
        };
        ui_kit::settings_section(ui, "vr_start", app.tr("prefs.vr_page.start"), |ui| {
            ui.horizontal_wrapped(|ui| {
                let response = setting::vr_headset(app, ui).backing_on_hover_text(app.tr("prefs.vr_headset_help"));
                crate::scenario::publish_target("prefs:vr_headset", response.rect);
                ui_kit::settings_badge(ui, app.tr("prefs.graphics_page.restart"), true);
            });
            if !status.is_empty() {
                ui_kit::text(ui, &status, ui_kit::Text::Hint("vr_status"));
            }
            connect_row(app, ui);
        });
        // How VR works: applied at once, also with the headset on, in sections of their own.
        crate::vr_settings::show(app, ui);
    });
}
