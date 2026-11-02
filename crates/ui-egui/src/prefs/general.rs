//! General preferences, composed from shared settings form blocks.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_page(ui, |ui| {
        let t = Tokens::get();
        let labels = t.space_24 * 6.0 + t.space_6;
        ui_kit::settings_heading_with_tag(ui, app.tr("prefs.general"), app.tr("prefs.general_note"), Some(crate::settings_profiles::tag));
        crate::settings_profiles::section(app, ui);
        ui_kit::settings_section(ui, "language", app.tr("ui.language"), |ui| {
            ui_kit::settings_form(ui, labels, |ui| {
                ui_kit::row(ui, app.tr("prefs.interface_language"), |ui| {
                    ui_kit::settings_control_width(ui, t.space_24 * 11.0 + t.space_16, |ui| {
                        ui_kit::dropdown_menu(
                            ui,
                            "pref_language",
                            &if app.preferences.language == crate::i18n::SYSTEM {
                                app.tr("prefs.language_system").to_owned()
                            } else {
                                crate::i18n::Locale::for_language(&app.preferences.language).display_name().into_owned()
                            },
                            |ui| {
                                if ui_kit::list_picker(
                                    ui,
                                    crate::i18n::SYSTEM,
                                    app.tr("prefs.language_system"),
                                    app.preferences.language == crate::i18n::SYSTEM,
                                )
                                .clicked()
                                {
                                    app.preferences.language = crate::i18n::SYSTEM.into();
                                }
                                for locale in crate::i18n::LOCALES {
                                    if ui_kit::list_picker(ui, locale.id, &locale.display_name(), app.preferences.language == locale.id).clicked() {
                                        app.preferences.language = locale.id.into();
                                    }
                                }
                            },
                        );
                    });
                });
            });
        });
        ui_kit::settings_section(ui, "saving", app.tr("prefs.saving"), |ui| {
            ui_kit::checkbox(ui, app.tr("Reopen the designs that were open when SolidCraft was last closed"), &mut app.preferences.reopen_designs);
            ui_kit::settings_form(ui, labels, |ui| {
                let shown = ui_kit::settings_row_help(ui, app.tr("ui.autosave_every"), crate::i18n::tr_label("0 turns it off"), |ui| {
                    let number = ui_kit::settings_number(ui, &mut app.autosave_minutes, 0.0..=120.0, 0.5);
                    number.widget_info(|| {
                        let mut info = egui::WidgetInfo::drag_value(ui.is_enabled(), app.autosave_minutes);
                        info.label = Some(app.tr("prefs.general.minutes").to_owned());
                        info
                    });
                    crate::scenario::publish_target("prefs:autosave_minutes", number.rect);
                    if number.changed() {
                        set_autosave(app);
                    }
                    ui_kit::settings_inline_note(ui, app.tr("prefs.minutes"));
                    ui.is_visible()
                });
                if shown {
                    ui_kit::below_row_control(ui, app.tr("ui.autosave_every"), t.space_24 * 10.0 + t.space_16 + t.space_4, |ui| {
                        ui_kit::settings_autosave_timeline(ui, app.autosave_minutes)
                    });
                }
            });
        });
        ui_kit::settings_section(ui, "files", app.tr("FILES"), |ui| {
            file_size_rows(app, ui, labels);
            ui.horizontal(|ui| {
                let width = (ui.available_width() - t.space_16 - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(egui::vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    setting::network_locations(app, ui);
                });
                ui_kit::help_icon(ui, app.tr("prefs.network_locations_note"));
            });
            if app.preferences.network_locations {
                let mut remove = None;
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(t.space_6, t.space_6);
                    for (i, host) in app.preferences.network_hosts.iter().enumerate() {
                        let label = format!("\\\\{host}");
                        let button = ui_kit::settings_chip(
                            ui,
                            &label,
                            ui_kit::SettingsChip {
                                key: Some(host),
                                monospace: true,
                                remove: Some(app.tr("prefs.network_host_remove")),
                                ..Default::default()
                            },
                        );
                        crate::scenario::publish_target(&format!("prefs:network_host:{host}:remove"), button.rect);
                        if button.clicked() {
                            remove = Some(i);
                        }
                    }
                });
                if let Some(i) = remove.filter(|i| *i < app.preferences.network_hosts.len()) {
                    app.preferences.network_hosts.remove(i);
                }
            }
            ui.horizontal(|ui| {
                let (single, tip) = (app.tr("ui.slicer_single_instance").to_string(), app.tr("ui.slicer_single_instance_tip").to_string());
                let width = (ui.available_width() - t.space_16 - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(egui::vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui_kit::checkbox(ui, &single, &mut app.preferences.slicer_single_instance).on_hover_text(&tip);
                });
                ui_kit::help_icon(ui, &tip);
            });
            ui_kit::settings_form(ui, labels, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.print_face_highlight"), app.tr("prefs.print_face_highlight_tip"), |ui| {
                    ui_kit::settings_control_width(ui, t.space_24 * 10.0, |ui| {
                        ui.spacing_mut().interact_size.x = t.space_24 + t.space_16 + t.space_4;
                        ui.spacing_mut().button_padding.x = t.space_4;
                        let response = ui_kit::slider(
                            ui,
                            &mut app.preferences.print_face_highlight,
                            0.0..=1.0,
                            ui_kit::Slider { step: Some(0.05), format: |v| format!("{:.0} %", v * 100.0), ..Default::default() },
                        );
                        crate::scenario::publish_target("prefs:print_face_highlight", response.rect);
                    });
                });
            });
        });
        ui_kit::settings_section(ui, "updates", app.tr("prefs.updates"), |ui| {
            crate::update::prefs_rows(app, ui);
        });
        ui_kit::settings_section(ui, "crash_reports", app.tr("prefs.general.crash_reports"), |ui| {
            ui.horizontal(|ui| {
                let count = app.crashes.saved_count();
                let label = crate::i18n::format("prefs.reports_saved", &[("n", &count.to_string())]);
                let tone = if count == 0 { ui_kit::SettingsBadgeTone::Neutral } else { ui_kit::SettingsBadgeTone::Warning };
                let badge = ui_kit::settings_state_badge(ui, &label, tone);
                crate::scenario::publish_rect("prefs:crash_count", badge.rect);
                if ui_kit::button(ui, app.tr("prefs.crash_reports"), ui_kit::Button::Normal)
                    .on_hover_text(app.tr("prefs.crash_reports_note"))
                    .clicked()
                {
                    crate::crash_ui::open(app);
                }
            });
        });
    });
}

/// Preferences › General › Largest file to open (#4086): the STEP/import and design file limits,
/// in MB. The browser keeps its limits, so there it is a note instead.
fn file_size_rows(app: &mut SolveApp, ui: &mut egui::Ui, labels: f32) {
    use solidcraft_engine::io::limits;
    if !limits::ADJUSTABLE {
        ui.horizontal(|ui| {
            ui_kit::text(ui, app.tr("prefs.max_file_size"), ui_kit::Text::Inline);
            ui_kit::help_icon(ui, app.tr("prefs.max_file_size_web"));
        });
        return;
    }
    let top = limits::ceiling() / limits::MB;
    let (step, design) = (app.tr("prefs.max_file_size_step"), app.tr("prefs.max_file_size_design"));
    let (step_caption, design_caption, group) = (app.tr("prefs.general.step_import"), app.tr("ui.design"), app.tr("prefs.max_file_size"));
    let mut help = app.tr("prefs.max_file_size_note").to_string();
    if let Some(bytes) = limits::override_bytes() {
        help.push('\n');
        help.push_str(&crate::i18n::format("prefs.max_file_size_flag", &[("size", &limits::describe(bytes))]));
    }
    let t = Tokens::get();
    let rows = [
        (step, step_caption, "prefs:max_step_file_mb", DEFAULT_STEP_MB, &mut app.preferences.max_step_file_mb),
        (design, design_caption, "prefs:max_design_file_mb", DEFAULT_DESIGN_MB, &mut app.preferences.max_design_file_mb),
    ];
    ui_kit::settings_block(ui, group, labels, |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
            ui.spacing_mut().item_spacing.x = t.space_16 + t.space_4;
            for (label, caption, handle, default, value) in rows {
                let size = limits::describe(value.saturating_mul(limits::MB));
                let unit = crate::i18n::tr("prefs.megabytes");
                let note = if *value >= 1024 { format!("{unit} ({size})") } else { unit.to_owned() };
                let number_width = t.space_24 * 3.0;
                let width = (number_width + t.space_4 + ui_kit::ContentWidth::label(ui, &note))
                    .max(ui_kit::ContentWidth::label(ui, caption))
                    .min(ui.max_rect().width());
                ui.allocate_ui_with_layout(
                    egui::vec2(width, t.control_height + t.space_16 + t.space_4),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = t.space_4;
                        ui_kit::settings_compound_field(ui, label, caption, |ui| {
                            let field = ui
                                .scope(|ui| {
                                    ui.spacing_mut().interact_size.x = number_width;
                                    ui_kit::number_field(ui, value, default..=top, 1.0, "")
                                })
                                .inner
                                .on_hover_text(&help);
                            crate::scenario::publish_rect(handle, field.rect);
                            let size = limits::describe(value.saturating_mul(limits::MB));
                            let note = if *value >= 1024 { format!("{unit} ({size})") } else { unit.to_owned() };
                            ui_kit::settings_inline_note(ui, &note);
                        });
                    },
                );
            }
        });
    });
}
