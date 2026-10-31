//! New-design defaults, with inline precision specimens.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_heading(ui, app.tr("prefs.design"), app.tr("prefs.design_note"));
    ui_kit::settings_section(ui, "osd_import", "OSD", |ui| {
        setting::osd_import_solids(app, ui);
        ui_kit::text(ui, app.tr("osd.import_solids_hint"), ui_kit::Text::Hint("osd_import"));
    });
    ui_kit::settings_section(ui, "units", app.tr("prefs.units_precision"), |ui| {
        ui_kit::settings_form(ui, t.space_24 * 7.0 + t.space_2, |ui| {
            ui_kit::row(ui, app.tr("ui.units_of_new_designs"), |ui| {
                let mut selected = UNITS.iter().position(|u| *u == app.preferences.default_units).unwrap_or(0);
                ui_kit::segmented(ui, &mut selected, UNITS, ui_kit::Segments { intrinsic: true, ..Default::default() });
                if let Some(unit) = UNITS.get(selected)
                    && app.preferences.default_units != *unit
                {
                    app.preferences.default_units = (*unit).into();
                    default_units_changed(app);
                }
            });
            ui_kit::row(ui, app.tr("ui.length_decimals"), |ui| {
                ui_kit::settings_number(ui, &mut app.preferences.length_decimals, 0..=6, 1.0);
                ui.add_space(t.space_6);
                let sample = format_length(
                    12.5 * unit_scale(&app.preferences.default_units).0,
                    &app.preferences.default_units,
                    app.preferences.length_decimals,
                );
                ui_kit::settings_precision_sample(ui, &sample);
            });
            ui_kit::row(ui, app.tr("ui.angle_decimals"), |ui| {
                ui_kit::settings_number(ui, &mut app.preferences.angle_decimals, 0..=6, 1.0);
                ui.add_space(t.space_6);
                ui_kit::settings_precision_sample(ui, &format!("{:.*}°", usize::from(app.preferences.angle_decimals.min(6)), 45.0));
            });
            ui_kit::row(ui, app.tr("prefs.parameter_list_rows"), |ui| {
                let tip = app.tr("prefs.parameter_list_rows_tip").to_string();
                ui_kit::settings_number(ui, &mut app.preferences.parameter_list_rows, PARAMETER_LIST_ROWS, 1.0).on_hover_text(tip);
            });
        });
        let tip = app.tr("ui.step_values_tip").to_string();
        let step = setting::step_values(app, ui).on_hover_text(tip);
        crate::scenario::publish_handle("prefs:step_values", step.rect.center());
        let tip = app.tr("prefs.value_boxes_in_view_tip").to_string();
        setting::value_boxes_in_view(app, ui).on_hover_text(tip);
    });
    ui_kit::settings_section(ui, "orientation", app.tr("prefs.orientation"), |ui| {
        ui.horizontal_wrapped(|ui| {
            for (id, label, y_up) in [("z", "Z up", false), ("y", "Y up", true)] {
                let subtitle = Some(if id == "z" { app.tr("prefs.default") } else { "" });
                if ui_kit::option_card(ui, app.tr_label(label), subtitle, app.preferences.up_axis == id, true, ui_kit::PreferenceFigure::UpAxis(y_up))
                    .clicked()
                {
                    app.preferences.up_axis = id.into();
                }
            }
        });
        ui_kit::text(
            ui,
            crate::i18n::tr_label("Which axis points up in the views (axis view, Home, Look At). Designs are unchanged."),
            ui_kit::Text::Hint("orientation"),
        );
        setting::offer_stand_up(app, ui);
        ui_kit::text(ui, app.tr("prefs.stand_up_note"), ui_kit::Text::Hint("stand_up"));
        // How a file from a program not known to save Y up is offered (#7717).
        let offered = app.preferences.offer_stand_up;
        ui_kit::settings_block_hint(
            ui,
            app.tr("prefs.stand_up_unknown"),
            app.tr("prefs.stand_up_unknown_note"),
            t.space_24 * 8.0 + t.space_8,
            |ui| {
                ui.add_enabled_ui(offered, |ui| {
                    let unknown = &mut app.preferences.stand_up_unknown;
                    ui_kit::settings_control_width(ui, t.space_24 * 7.0 + t.space_12, |ui| {
                        ui_kit::dropdown_menu(ui, "pref_stand_up_unknown", crate::i18n::tr(unknown.label_id()), |ui| {
                            for choice in StandUpUnknown::ALL {
                                if ui_kit::list_picker(ui, choice.label_id(), crate::i18n::tr(choice.label_id()), *unknown == choice).clicked() {
                                    *unknown = choice;
                                }
                            }
                        });
                    });
                });
            },
        );
    });
    origin_plane_names(app, ui);
    ui_kit::settings_section(ui, "linked_designs", app.tr("prefs.linked_designs"), |ui| {
        setting::update_changed_links(app, ui);
        setting::double_click_components(app, ui);
        setting::insert_lists_open_designs(app, ui);
    });
}

/// Origin plane names: each choice shows the three names it gives (`XY / XZ / YZ`), with a note
/// that only the names shown change.
fn origin_plane_names(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::settings_section(ui, "origin_plane_names", app.tr("prefs.origin_plane_names"), |ui| {
        let choices: Vec<String> =
            OriginPlaneNames::ALL.iter().map(|names| OriginPlaneNames::PLANES.map(|plane| names.label(plane)).join(" / ")).collect();
        let options: Vec<&str> = choices.iter().map(String::as_str).collect();
        let mut selected = OriginPlaneNames::ALL.iter().position(|names| *names == app.preferences.origin_plane_names).unwrap_or(0);
        if ui_kit::settings_segments(ui, app.tr("prefs.origin_plane_names"), &mut selected, &options)
            && let Some(names) = OriginPlaneNames::ALL.get(selected)
        {
            app.preferences.origin_plane_names = *names;
        }
        // Handles that don't depend on the language, by the choice's saved value.
        for (choice, key) in choices.iter().zip(["xyz", "top_front_right"]) {
            if let Some(at) = crate::scenario::handle_at(&format!("dialog:{}:{choice}", app.tr("prefs.origin_plane_names"))) {
                crate::scenario::publish_handle(&format!("prefs:origin_plane_names:{key}"), at);
            }
        }
        let top = OriginPlaneNames::TopFrontRight;
        let note =
            crate::i18n::format("prefs.origin_plane_names_note", &[("top", top.label("XY")), ("front", top.label("XZ")), ("right", top.label("YZ"))]);
        ui_kit::text(ui, &note, ui_kit::Text::Hint("origin_plane_names"));
        setting::hide_origin(app, ui);
        ui_kit::text(ui, app.tr("ui.hide_origin_note"), ui_kit::Text::Hint("hide_origin"));
    });
}
