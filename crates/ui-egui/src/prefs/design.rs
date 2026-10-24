//! New-design defaults, with inline precision specimens.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_heading(ui, app.tr("prefs.design"), app.tr("prefs.design_note"));
    let response = setting::joint_moving_first(app, ui);
    crate::scenario::publish_rect("prefs:joint_moving_first", response.rect);
    ui_kit::text(ui, app.tr("settings.joint_moving_first_help"), ui_kit::Text::Note("joint_order"));
    ui_kit::settings_section(ui, "units", app.tr("prefs.units_precision"), |ui| {
        ui_kit::settings_form(ui, t.space_24 * 7.0 + t.space_2, |ui| {
            ui_kit::row(ui, app.tr("ui.units_of_new_designs"), |ui| {
                let mut selected = UNITS.iter().position(|u| *u == app.preferences.default_units).unwrap_or(0);
                ui_kit::segmented(ui, &mut selected, &UNITS, ui_kit::Segments { intrinsic: true, ..Default::default() });
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
        });
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
        setting::extrude_auto_to_object(app, ui);
    });
    // The Hole command's own settings, under its name.
    ui_kit::settings_section(ui, "create_hole", crate::i18n::tr_label("Create Hole"), |ui| {
        setting::hole_snaps(app, ui);
        ui_kit::text(
            ui,
            crate::i18n::tr_label("A Single hole snaps to the middle of its face and to the centres of its round edges"),
            ui_kit::Text::Hint("hole_snaps"),
        );
        setting::hole_moves(app, ui);
        ui_kit::text(
            ui,
            crate::i18n::tr_label(
                "A placed Single hole has a handle to drag it, and picking its face again moves the hole there instead of taking it out",
            ),
            ui_kit::Text::Hint("hole_moves"),
        );
    });
}
