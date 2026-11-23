//! Privacy preferences: anonymous usage statistics (`usage_stats`, docs/privacy.md).
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.privacy"), app.tr("prefs.privacy_note"));
        ui_kit::settings_section(ui, "usage_statistics", app.tr("prefs.privacy.usage"), |ui| usage(app, ui));
    });
}

/// A build without the `usage-stats` feature has none of it.
#[cfg(not(feature = "usage-stats"))]
fn usage(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_inline_note(ui, app.tr("prefs.privacy.not_included"));
}

#[cfg(feature = "usage-stats")]
fn usage(app: &mut SolveApp, ui: &mut egui::Ui) {
    use crate::usage_stats::{self, Off};
    let off = usage_stats::off(app);
    let label = app.tr("prefs.privacy.share").to_string();
    match off {
        // The organization's or the environment's answer shows in place of the person's, which is
        // kept for when it goes.
        Some(reason @ (Off::Policy | Off::Environment)) => {
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(false, |ui| {
                    let check = ui_kit::checkbox(ui, &label, &mut false);
                    crate::scenario::publish_target("prefs:usage_statistics", check.rect);
                });
                let why = if reason == Off::Policy { "policy.set_by_organization" } else { "prefs.privacy.env_off" };
                ui_kit::settings_inline_note(ui, app.tr(why));
            });
        }
        _ => {
            let check = ui_kit::checkbox(ui, &label, &mut app.preferences.usage_statistics);
            crate::scenario::publish_target("prefs:usage_statistics", check.rect);
            // Switched on here is an explicit yes, like OK on the notice; off ends the notice too.
            if check.changed() {
                if app.preferences.usage_statistics {
                    usage_stats::agree(app);
                } else {
                    app.usage.store.days.clear();
                }
            }
        }
    }
    // Wrapped: the note is a few lines long.
    ui.horizontal_wrapped(|ui| ui_kit::settings_inline_note(ui, app.tr("prefs.privacy.share_help")));
    if off.is_none() && !app.usage.store.notice_ok {
        ui.horizontal_wrapped(|ui| ui_kit::settings_inline_note(ui, app.tr("prefs.privacy.waiting_ok")));
    }
    let on = off.is_none();
    // The installation id the reports carry, and what can be done with it.
    let id = if app.usage.store.install_id.is_empty() { app.tr("prefs.privacy.no_id").to_string() } else { app.usage.store.install_id.clone() };
    ui.horizontal_wrapped(|ui| {
        ui_kit::settings_inline_note(ui, app.tr("prefs.privacy.install_id"));
        let shown = ui.add(egui::Label::new(egui::RichText::new(&id).monospace()).selectable(true));
        crate::scenario::publish_target("prefs:usage_statistics:install_id", shown.rect);
    });
    ui.horizontal_wrapped(|ui| {
        let reset = ui_kit::button(ui, app.tr("prefs.privacy.reset_id"), ui_kit::Button::Enabled(on));
        crate::scenario::publish_target("prefs:usage_statistics:reset_id", reset.rect);
        if on && reset.clicked() {
            usage_stats::reset_id(app);
            let text = app.tr("prefs.privacy.reset_done").to_string();
            app.set_status(text, false);
        }
        // Delete my data works with statistics off too (data sent earlier stays until deleted);
        // not when the organization or the environment turned them off, which sends nothing.
        // One at a time: while a deletion is on its way, the button waits for it.
        let can_delete = !matches!(off, Some(Off::Policy | Off::Environment)) && app.usage.store.forget.is_empty();
        let delete = ui_kit::button(ui, app.tr("prefs.privacy.delete"), ui_kit::Button::Enabled(can_delete));
        crate::scenario::publish_target("prefs:usage_statistics:delete", delete.rect);
        if can_delete && delete.clicked() && usage_stats::delete_my_data(app) {
            let text = solidcraft_ui_text::format("prefs.privacy.delete_requested", &[("id", &app.usage.store.install_id)]);
            app.set_status(text, false);
        }
        let label = app.tr(if app.usage.show_report { "prefs.privacy.hide" } else { "prefs.privacy.show" }).to_string();
        let show = ui_kit::button(ui, &label, ui_kit::Button::Normal);
        crate::scenario::publish_target("prefs:usage_statistics:show", show.rect);
        if show.clicked() {
            app.usage.show_report = !app.usage.show_report;
        }
    });
    // Said in place too, so nobody wonders whether the click worked.
    if !app.usage.store.forget.is_empty() {
        let text = solidcraft_ui_text::format("prefs.privacy.delete_requested", &[("id", &app.usage.store.install_id)]);
        ui.horizontal_wrapped(|ui| ui_kit::settings_inline_note(ui, &text));
    }
    if app.usage.show_report {
        match usage_stats::next_report_json(app) {
            Some(json) => {
                let shown = ui_kit::read_only_text(ui, &json);
                crate::scenario::publish_target("prefs:usage_statistics:report", shown.rect);
            }
            None => {
                ui_kit::settings_inline_note(ui, app.tr("prefs.privacy.nothing"));
            }
        }
    }
}
