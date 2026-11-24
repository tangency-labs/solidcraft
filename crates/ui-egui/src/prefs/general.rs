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
                let locked = app.policy.locks("language");
                ui_kit::row(ui, app.tr("prefs.interface_language"), |ui| {
                    crate::policy::row(ui, locked, |ui| {
                        ui_kit::settings_control_width(ui, t.space_24 * 11.0 + t.space_16, |ui| {
                            ui_kit::dropdown_menu(
                                ui,
                                "pref_language",
                                &if app.preferences.language == solidcraft_ui_text::SYSTEM {
                                    app.tr("prefs.language_system").to_owned()
                                } else {
                                    solidcraft_ui_text::Locale::for_language(&app.preferences.language).display_name().into_owned()
                                },
                                |ui| {
                                    if ui_kit::list_picker(
                                        ui,
                                        solidcraft_ui_text::SYSTEM,
                                        app.tr("prefs.language_system"),
                                        app.preferences.language == solidcraft_ui_text::SYSTEM,
                                    )
                                    .clicked()
                                    {
                                        app.preferences.language = solidcraft_ui_text::SYSTEM.into();
                                    }
                                    for locale in crate::i18n::LOCALES {
                                        if ui_kit::list_picker(ui, locale.id, &locale.display_name(), app.preferences.language == locale.id).clicked()
                                        {
                                            app.preferences.language = locale.id.into();
                                        }
                                    }
                                },
                            );
                        })
                    });
                });
            });
        });
        // What New makes (#6892): one design for everything, or a file of its own for each part,
        // assembly (a part may hold sheet metal). Files already made keep their kind.
        ui_kit::settings_section(ui, "environment", app.tr("prefs.environment"), |ui| {
            ui_kit::settings_form(ui, labels, |ui| {
                ui_kit::row(ui, app.tr("prefs.environment"), |ui| {
                    let shown = crate::prefs::parts_and_assemblies(&app.preferences);
                    let selected = app.tr(if shown { "prefs.environment_parts" } else { "prefs.environment_single" }).to_string();
                    let choice = ui_kit::dropdown_menu(ui, "pref_environment", &selected, |ui| {
                        for (value, key) in [
                            (crate::prefs::ENVIRONMENT_SINGLE, "prefs.environment_single"),
                            (crate::prefs::ENVIRONMENT_PARTS_AND_ASSEMBLIES, "prefs.environment_parts"),
                        ] {
                            let picked = (value == crate::prefs::ENVIRONMENT_PARTS_AND_ASSEMBLIES) == shown;
                            let response = ui_kit::list_picker(ui, key, solidcraft_ui_text::tr(key), picked);
                            crate::scenario::publish_rect(&format!("prefs:environment_{value}"), response.rect);
                            if response.clicked() {
                                app.preferences.environment = value.into();
                                ui.close();
                            }
                        }
                    });
                    crate::scenario::publish_rect("prefs:environment", choice.rect);
                });
                ui_kit::text(ui, app.tr("prefs.environment_note"), ui_kit::Text::Note("environment"));
                // How an assembly takes its linked parts' changes (#6892): Live reads them when its
                // tab comes back or it opens; Update active names them and waits for Update Links.
                // It is the per-user `update_changed_links`, also on the Design page.
                if crate::prefs::parts_and_assemblies(&app.preferences) {
                    ui_kit::row(ui, app.tr("prefs.update_linked_parts"), |ui| {
                        let live = app.preferences.update_changed_links;
                        let selected = app.tr(if live { "prefs.update_live" } else { "prefs.update_active" }).to_string();
                        let choice = ui_kit::dropdown_menu(ui, "pref_update_linked_parts", &selected, |ui| {
                            for (on, key) in [(true, "prefs.update_live"), (false, "prefs.update_active")] {
                                let response = ui_kit::list_picker(ui, key, solidcraft_ui_text::tr(key), live == on);
                                crate::scenario::publish_rect(if on { "prefs:update_live" } else { "prefs:update_active" }, response.rect);
                                if response.clicked() {
                                    app.preferences.update_changed_links = on;
                                    ui.close();
                                }
                            }
                        });
                        crate::scenario::publish_rect("prefs:update_linked_parts", choice.rect);
                    });
                    ui_kit::text(ui, app.tr("prefs.update_linked_parts_note"), ui_kit::Text::Note("update_linked_parts"));
                }
            });
        });
        ui_kit::settings_section(ui, "startup", app.tr("ui.on_startup"), |ui| {
            ui_kit::settings_form(ui, labels, |ui| {
                ui_kit::row(ui, app.tr("ui.on_startup"), |ui| {
                    let selected = app.tr(if app.preferences.show_home_on_startup { "ui.home" } else { "ui.new_design" }).to_string();
                    let choice = ui_kit::dropdown_menu(ui, "pref_startup", &selected, |ui| {
                        for (home, key) in [(true, "ui.home"), (false, "ui.new_design")] {
                            let response = ui_kit::list_picker(ui, key, solidcraft_ui_text::tr(key), app.preferences.show_home_on_startup == home);
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
        ui_kit::settings_section(ui, "saving", app.tr("prefs.saving"), |ui| {
            let locked = app.policy.locks("reopen_designs");
            let reopen = crate::policy::row(ui, locked, |ui| {
                ui_kit::checkbox(ui, app.tr("Reopen the designs that were open when SolidCraft was last closed"), &mut app.preferences.reopen_designs)
            });
            crate::scenario::publish_target("prefs:reopen_designs", reopen.rect);
            // Only the web app keeps designs in the browser (#10220).
            if app.services.browser_files.is_some() {
                let tip = app.tr("prefs.keep_unsaved_in_browser_tip").to_string();
                ui_kit::checkbox(ui, app.tr("prefs.keep_unsaved_in_browser"), &mut app.preferences.keep_unsaved_in_browser)
                    .backing_on_hover_text(tip);
                if app.preferences.keep_unsaved_in_browser {
                    let tip = app.tr("prefs.keep_unsaved_also_asks_tip").to_string();
                    ui_kit::checkbox(ui, app.tr("prefs.keep_unsaved_also_asks"), &mut app.preferences.keep_unsaved_also_asks)
                        .backing_on_hover_text(tip);
                }
            }
            setting::save_prompt_one_at_a_time(app, ui).on_hover_text(app.tr("prefs.save_prompt_one_at_a_time_help"));
            setting::experimental_smaller_saves(app, ui).on_hover_text(app.tr("prefs.experimental_smaller_saves_help"));
            ui_kit::settings_form(ui, labels, |ui| {
                let shown = ui_kit::settings_row_help(ui, app.tr("ui.autosave_every"), solidcraft_ui_text::tr_label("0 turns it off"), |ui| {
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
                        super::figures::autosave_timeline(ui, app.autosave_minutes)
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
            link_folders(app, ui);
            ui.horizontal(|ui| {
                let (single, tip) = (app.tr("ui.slicer_single_instance").to_string(), app.tr("ui.slicer_single_instance_tip").to_string());
                let width = (ui.available_width() - t.space_16 - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(egui::vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui_kit::checkbox(ui, &single, &mut app.preferences.slicer_single_instance).backing_on_hover_text(&tip);
                });
                ui_kit::help_icon(ui, &tip);
            });
            ui.horizontal(|ui| {
                let (only, tip) = (app.tr("ui.export_selects_only_body").to_string(), app.tr("ui.export_selects_only_body_tip").to_string());
                let width = (ui.available_width() - t.space_16 - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(egui::vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui_kit::checkbox(ui, &only, &mut app.preferences.export_selects_only_body).backing_on_hover_text(&tip);
                });
                ui_kit::help_icon(ui, &tip);
            });
            ui.horizontal(|ui| {
                let (keep, tip) = (app.tr("prefs.keep_print_faces").to_string(), app.tr("prefs.keep_print_faces_tip").to_string());
                let width = (ui.available_width() - t.space_16 - ui.spacing().item_spacing.x).max(0.0);
                ui.allocate_ui_with_layout(egui::vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let response = ui_kit::checkbox(ui, &keep, &mut app.preferences.keep_print_faces).backing_on_hover_text(&tip);
                    crate::scenario::publish_target("prefs:keep_print_faces", response.rect);
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
        ui_kit::settings_section(ui, "freecad", app.tr("prefs.freecad_files"), |ui| {
            setting::freecad_history(app, ui);
        });
        ui_kit::settings_section(ui, "cadmpeg", app.tr("prefs.cadmpeg_import"), |ui| {
            let cadmpeg = setting::cadmpeg_import(app, ui);
            crate::scenario::publish_target("prefs:cadmpeg_import", cadmpeg.rect);
        });
        ui_kit::settings_section(ui, "cadmpeg_export", app.tr("prefs.cadmpeg_export"), |ui| {
            let cadmpeg = setting::cadmpeg_export(app, ui);
            crate::scenario::publish_target("prefs:cadmpeg_export", cadmpeg.rect);
        });
        ui_kit::settings_section(ui, "dialogs", app.tr("prefs.dialogs"), |ui| {
            let right_click = setting::right_click_confirms(app, ui);
            crate::scenario::publish_target("prefs:right_click_confirms", right_click.rect);
            let import_options = setting::ask_import_options(app, ui);
            crate::scenario::publish_target("prefs:ask_import_options", import_options.rect);
            let first_letter = setting::dialog_first_letter_buttons(app, ui).backing_on_hover_text(app.tr("prefs.dialog_first_letter_buttons_help"));
            crate::scenario::publish_target("prefs:dialog_first_letter_buttons", first_letter.rect);
        });
        ui_kit::settings_section(ui, "community", app.tr("prefs.community"), |ui| {
            let locked = app.policy.locks("show_donate_button");
            let donate = crate::policy::row(ui, locked, |ui| setting::show_donate_button(app, ui))
                .backing_on_hover_text(app.tr("prefs.show_donate_button_help"));
            crate::scenario::publish_target("prefs:show_donate_button", donate.rect);
            let locked = app.policy.locks("show_discord_button");
            let discord = crate::policy::row(ui, locked, |ui| setting::show_discord_button(app, ui))
                .backing_on_hover_text(app.tr("prefs.show_discord_button_help"));
            crate::scenario::publish_target("prefs:show_discord_button", discord.rect);
        });
        ui_kit::settings_section(ui, "updates", app.tr("prefs.updates"), |ui| {
            crate::update::prefs_rows(app, ui);
        });
        ui_kit::settings_section(ui, "crash_reports", app.tr("prefs.general.crash_reports"), |ui| {
            ui.horizontal(|ui| {
                let count = app.crashes.saved_count();
                let label = solidcraft_ui_text::format("prefs.reports_saved", &[("n", &count.to_string())]);
                let tone = if count == 0 { ui_kit::SettingsBadgeTone::Neutral } else { ui_kit::SettingsBadgeTone::Warning };
                let badge = ui_kit::settings_state_badge(ui, &label, tone);
                crate::scenario::publish_rect("prefs:crash_count", badge.rect);
                if ui_kit::button(ui, app.tr("prefs.crash_reports"), ui_kit::Button::Normal)
                    .backing_on_hover_text(app.tr("prefs.crash_reports_note"))
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
        help.push_str(&solidcraft_ui_text::format("prefs.max_file_size_flag", &[("size", &limits::describe(bytes))]));
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
                let unit = solidcraft_ui_text::tr("prefs.megabytes");
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
                                .backing_on_hover_text(&help);
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

/// The folders linked files load from when a design opens (`linked_file_folders`): each can be
/// removed, or all forgotten; a design in one of them that is removed asks again.
fn link_folders(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    // Wrapped, so a narrow window keeps the notes inside the page.
    ui.horizontal_wrapped(|ui| ui_kit::settings_inline_note(ui, app.tr("prefs.link_folders_note")));
    if app.preferences.linked_file_folders.is_empty() {
        ui.horizontal_wrapped(|ui| ui_kit::settings_inline_note(ui, app.tr("prefs.link_folders_none")));
        return;
    }
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(t.space_6, t.space_6);
        for (i, folder) in app.preferences.linked_file_folders.iter().enumerate() {
            let chip =
                ui_kit::SettingsChip { key: Some(folder), monospace: true, remove: Some(app.tr("prefs.link_folders_remove")), ..Default::default() };
            let button = ui_kit::settings_chip(ui, folder, chip);
            crate::scenario::publish_target(&format!("prefs:link_folder:{i}:remove"), button.rect);
            if button.clicked() {
                remove = Some(i);
            }
        }
    });
    if let Some(i) = remove.filter(|i| *i < app.preferences.linked_file_folders.len()) {
        app.preferences.linked_file_folders.remove(i);
    }
    let clear = ui_kit::button(ui, app.tr("prefs.link_folders_clear"), ui_kit::Button::Normal);
    crate::scenario::publish_target("prefs:link_folders:clear", clear.rect);
    if clear.clicked() {
        app.preferences.linked_file_folders.clear();
    }
}
