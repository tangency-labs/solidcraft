//! New-design defaults, with inline precision specimens.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let label_width = [
        "ui.units_of_new_designs",
        "ui.length_decimals",
        "ui.angle_decimals",
        "prefs.parameter_list_rows",
        "prefs.origin_plane_names",
        "prefs.stand_up_unknown",
    ]
    .into_iter()
    .map(|id| ui_kit::ContentWidth::label(ui, app.tr(id)))
    .fold(0.0, f32::max)
        + t.space_16
        + t.space_4;
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.design"), app.tr("prefs.design_note"));
        ui_kit::settings_section(ui, "units", app.tr("prefs.units_precision"), |ui| {
            ui_kit::settings_form(ui, label_width, |ui| {
                ui_kit::row(ui, app.tr("ui.units_of_new_designs"), |ui| {
                    let mut selected = UNITS.iter().position(|u| *u == app.preferences.default_units).unwrap_or(0);
                    ui_kit::segmented(
                        ui,
                        &mut selected,
                        UNITS,
                        ui_kit::Segments {
                            intrinsic: true,
                            targets: &[
                                "prefs:default_units:mm",
                                "prefs:default_units:cm",
                                "prefs:default_units:m",
                                "prefs:default_units:in",
                                "prefs:default_units:ft",
                            ],
                            ..Default::default()
                        },
                    );
                    if let Some(unit) = UNITS.get(selected)
                        && app.preferences.default_units != *unit
                    {
                        app.preferences.default_units = (*unit).into();
                        default_units_changed(app);
                    }
                });
                ui_kit::row(ui, app.tr("ui.length_decimals"), |ui| {
                    ui.scope(|ui| {
                        ui.spacing_mut().interact_size.x = t.space_24 * 2.0 + t.space_8;
                        let response = ui_kit::number_field(ui, &mut app.preferences.length_decimals, 0..=6, 1.0, "");
                        crate::scenario::publish_target("prefs:length_decimals", response.rect);
                    });
                    ui.add_space(t.space_6);
                    let sample = format_length(
                        12.5 * unit_scale(&app.preferences.default_units).0,
                        &app.preferences.default_units,
                        app.preferences.length_decimals,
                    );
                    ui_kit::settings_precision_sample(ui, &sample);
                });
                ui_kit::row(ui, app.tr("ui.angle_decimals"), |ui| {
                    ui.scope(|ui| {
                        ui.spacing_mut().interact_size.x = t.space_24 * 2.0 + t.space_8;
                        let response = ui_kit::number_field(ui, &mut app.preferences.angle_decimals, 0..=6, 1.0, "");
                        crate::scenario::publish_target("prefs:angle_decimals", response.rect);
                    });
                    ui.add_space(t.space_6);
                    ui_kit::settings_precision_sample(ui, &format!("{:.*}°", usize::from(app.preferences.angle_decimals.min(6)), 45.0));
                });
                ui_kit::settings_row_help(ui, app.tr("prefs.parameter_list_rows"), app.tr("prefs.parameter_list_rows_tip"), |ui| {
                    let response = ui_kit::named_control(ui, app.tr("prefs.design_tools.rows"), |ui| {
                        ui_kit::settings_number(ui, &mut app.preferences.parameter_list_rows, PARAMETER_LIST_ROWS, 1.0)
                    });
                    crate::scenario::publish_target("prefs:parameter_list_rows", response.rect);
                    crate::scenario::publish_target(&format!("dialog:{}", app.tr("prefs.parameter_list_rows")), response.rect);
                });
            });
            for (setting, help, target, legacy) in [
                (
                    setting::step_values as fn(&mut SolveApp, &mut egui::Ui) -> egui::Response,
                    "ui.step_values_tip",
                    "step_values",
                    "Step values with the arrow keys and the wheel",
                ),
                (
                    setting::value_boxes_in_view,
                    "prefs.value_boxes_in_view_tip",
                    "value_boxes_in_view",
                    "Show values beside their handles in the view",
                ),
            ] {
                ui.horizontal_wrapped(|ui| {
                    let response = setting(app, ui).on_hover_text(app.tr(help));
                    crate::scenario::publish_target(&format!("prefs:{target}"), response.rect);
                    crate::scenario::publish_target(&format!("dialog:{legacy}"), response.rect);
                    if !crate::prefs_search::suppress_chrome(ui) {
                        ui_kit::help_icon(ui, app.tr(help));
                    }
                });
            }
        });
        ui_kit::settings_section(ui, "orientation", app.tr("prefs.orientation"), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = t.space_10;
                for (id, label, y_up) in [("z", "Z up", false), ("y", "Y up", true)] {
                    let subtitle = Some(if id == "z" { app.tr("prefs.default") } else { "" });
                    let response = ui_kit::option_card(
                        ui,
                        app.tr_label(label),
                        subtitle,
                        app.preferences.up_axis == id,
                        true,
                        ui_kit::PreferenceFigure::UpAxis(y_up),
                    );
                    crate::scenario::publish_target(&format!("prefs:up_axis:{id}"), response.rect);
                    if response.clicked() {
                        app.preferences.up_axis = id.into();
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                let response = setting::offer_stand_up(app, ui).on_hover_text(app.tr("prefs.stand_up_note"));
                crate::scenario::publish_target("prefs:offer_stand_up", response.rect);
                if !crate::prefs_search::suppress_chrome(ui) {
                    ui_kit::help_icon(ui, app.tr("prefs.stand_up_note"));
                }
            });
        });
        ui_kit::settings_section(ui, "imported_files", app.tr("prefs.design_tools.imported_files"), |ui| {
            ui_kit::settings_form(ui, label_width, |ui| {
                ui_kit::settings_row_help(ui, app.tr("prefs.stand_up_unknown"), app.tr("prefs.stand_up_unknown_note"), |ui| {
                    let offered = app.preferences.offer_stand_up;
                    ui.add_enabled_ui(offered, |ui| {
                        ui_kit::settings_control_width(ui, t.space_24 * 8.0 + t.space_8, |ui| {
                            let mut selected = StandUpUnknown::ALL.iter().position(|v| *v == app.preferences.stand_up_unknown).unwrap_or(1);
                            let options = [app.tr("prefs.design_tools.ask"), app.tr("prefs.design_tools.note"), app.tr("prefs.design_tools.off")];
                            let response = ui_kit::dropdown(ui, "pref_stand_up_unknown", &mut selected, &options);
                            crate::scenario::publish_target("prefs:stand_up_unknown", response.rect);
                            crate::scenario::publish_target("dialog:For files from unknown programs", response.rect);
                            for (choice, option) in StandUpUnknown::ALL.iter().zip(options) {
                                if let Some(rect) = crate::scenario::rect_of(&format!("dialog:{}:{option}", app.tr("prefs.stand_up_unknown"))) {
                                    crate::scenario::publish_target(&format!("dialog:list:{}", choice.label_id()), rect);
                                }
                            }
                            if let Some(choice) = StandUpUnknown::ALL.get(selected) {
                                app.preferences.stand_up_unknown = *choice;
                            }
                        });
                    });
                });
            });
        });
        origin_plane_names(app, ui, label_width);
        ui_kit::settings_section(ui, "linked_designs", app.tr("prefs.linked_designs"), |ui| {
            let response = setting::update_changed_links(app, ui);
            crate::scenario::publish_target("prefs:update_changed_links", response.rect);
            crate::scenario::publish_target("dialog:Update changed linked designs when a tab comes back", response.rect);
            ui.horizontal_wrapped(|ui| {
                let response = setting::double_click_components(app, ui).on_hover_text(app.tr("prefs.design_tools.double_click_help"));
                crate::scenario::publish_target("prefs:double_click_components", response.rect);
                crate::scenario::publish_target("dialog:Double-click a component to activate it, or to edit a linked one in place", response.rect);
                if !crate::prefs_search::suppress_chrome(ui) {
                    ui_kit::help_icon(ui, app.tr("prefs.design_tools.double_click_help"));
                }
            });
            let response = setting::insert_lists_open_designs(app, ui);
            crate::scenario::publish_target("prefs:insert_lists_open_designs", response.rect);
            crate::scenario::publish_target("dialog:Insert Design lists the designs open in other tabs first", response.rect);
        });
    });
}

/// These choices change only displayed names; the saved plane IDs remain XY, XZ and YZ.
fn origin_plane_names(app: &mut SolveApp, ui: &mut egui::Ui, label_width: f32) {
    ui_kit::settings_section(ui, "origin_plane_names", app.tr("ui.origin"), |ui| {
        let choices: Vec<String> =
            OriginPlaneNames::ALL.iter().map(|names| OriginPlaneNames::PLANES.map(|plane| names.label(plane)).join(" . ")).collect();
        let options: Vec<&str> = choices.iter().map(String::as_str).collect();
        ui_kit::settings_form(ui, label_width, |ui| {
            ui_kit::row(ui, app.tr("prefs.origin_plane_names"), |ui| {
                let mut selected = OriginPlaneNames::ALL.iter().position(|names| *names == app.preferences.origin_plane_names).unwrap_or(0);
                if ui_kit::segmented(
                    ui,
                    &mut selected,
                    &options,
                    ui_kit::Segments {
                        intrinsic: true,
                        targets: &["prefs:origin_plane_names:xyz", "prefs:origin_plane_names:top_front_right"],
                        ..Default::default()
                    },
                ) && let Some(names) = OriginPlaneNames::ALL.get(selected)
                {
                    app.preferences.origin_plane_names = *names;
                }
                for (choice, key) in choices.iter().zip(["xyz", "top_front_right"]) {
                    if let Some(rect) = crate::scenario::rect_of(&format!("prefs:origin_plane_names:{key}")) {
                        crate::scenario::publish_target(
                            &format!("dialog:{}:{}", app.tr("prefs.origin_plane_names"), choice.replace(" . ", " / ")),
                            rect,
                        );
                    }
                }
            });
        });
        let response = setting::hide_origin(app, ui);
        crate::scenario::publish_target("prefs:hide_origin", response.rect);
        crate::scenario::publish_target("dialog:Hide origin by default", response.rect);
    });
}
