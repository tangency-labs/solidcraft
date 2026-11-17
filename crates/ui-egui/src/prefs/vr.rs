//! VR headset preferences (docs/vr.md): whether the app starts in VR, and what this run
//! is doing about it. Few people have a headset, so VR keeps its settings on this page of its
//! own, in the Input group after the 3D mouse, and the settings VR gains go here too.
use super::*;

/// The VR headset page. Where the host has no VR (macOS, the browser), it only says what VR needs.
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
        });
    });
}
