//! General preferences, composed from shared settings form blocks.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let labels = t.space_24 * 6.0 + t.space_6;
    ui_kit::settings_heading(ui, app.tr("prefs.general"), app.tr("prefs.general_note"));
    ui_kit::settings_section(ui, "startup", app.tr("ui.on_startup"), |ui| {
        ui_kit::settings_form(ui, labels, |ui| {
            ui_kit::row(ui, app.tr("ui.on_startup"), |ui| {
                let selected = app.tr(if app.preferences.show_home_on_startup { "ui.homeView" } else { "ui.new_design" }).to_string();
                let choice = ui_kit::dropdown_menu(ui, "pref_startup", &selected, |ui| {
                    for (home, key) in [(true, "ui.homeView"), (false, "ui.new_design")] {
                        let response = ui_kit::list_picker(ui, key, crate::i18n::tr(key), app.preferences.show_home_on_startup == home);
                        crate::scenario::publish_rect(if home { "prefs:startup_home" } else { "prefs:startup_design" }, response.rect);
                        if response.clicked() {
                            app.preferences.show_home_on_startup = home;
                            ui.close();
                        }
                    }
                });
                crate::scenario::publish_rect("prefs:startup", choice.rect);
            });
            ui_kit::text(ui, app.tr("ui.on_startup_note"), ui_kit::Text::Note("startup"));
        });
    });
    ui_kit::settings_section(ui, "language", app.tr("ui.language"), |ui| {
        ui_kit::settings_form(ui, labels, |ui| {
            ui_kit::row(ui, app.tr("prefs.interface_language"), |ui| {
                ui_kit::settings_control_width(ui, t.form_card_width - t.space_4, |ui| {
                    ui_kit::dropdown_menu(ui, "pref_language", crate::i18n::Locale::for_language(&app.preferences.language).name(), |ui| {
                        for locale in crate::i18n::LOCALES {
                            if ui_kit::list_picker(ui, locale.id, locale.name, app.preferences.language == locale.id).clicked() {
                                app.preferences.language = locale.id.into();
                            }
                        }
                    });
                });
            });
        });
    });
    ui_kit::settings_section(ui, "saving", app.tr("prefs.saving"), |ui| {
        ui_kit::settings_block_hint(ui, app.tr("ui.autosave_every"), crate::i18n::tr_label("0 turns it off"), labels, |ui| {
            ui.horizontal(|ui| {
                if ui_kit::settings_number(ui, &mut app.autosave_minutes, 0.0..=120.0, 0.5).changed() {
                    set_autosave(app);
                }
                ui_kit::settings_inline_note(ui, app.tr("prefs.minutes"));
            });
            ui_kit::settings_autosave_timeline(ui, app.autosave_minutes);
        });
        ui_kit::settings_block(ui, app.tr_label("Crash reports"), labels, |ui| {
            ui.horizontal(|ui| {
                let count = app.crashes.saved_count();
                let label = crate::i18n::format("prefs.reports_saved", &[("n", &count.to_string())]);
                ui_kit::settings_badge(ui, &label, count > 0);
                if ui_kit::button(ui, app.tr("prefs.crash_reports"), ui_kit::Button::Normal).clicked() {
                    crate::crash_ui::open(app);
                }
            });
            ui_kit::text(ui, app.tr("prefs.crash_reports_note"), ui_kit::Text::Hint("crash_reports"));
        });
    });
    ui_kit::settings_section(ui, "updates", app.tr("prefs.updates"), |ui| {
        ui_kit::settings_block(ui, app.tr("update.prefs_label"), labels, |ui| crate::update::prefs_rows(app, ui));
    });
}
