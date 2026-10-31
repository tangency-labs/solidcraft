//! New-design defaults, with inline precision specimens.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    ui_kit::settings_heading(ui, app.tr("prefs.design"), app.tr("prefs.design_note"));
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
        let tip = app.tr("ui.step_values_tip").to_string();
        let step = setting::step_values(app, ui).on_hover_text(tip);
        crate::scenario::publish_handle("prefs:step_values", step.rect.center());
    });
    // When SMLib's full check runs while features are computed (`diagnostics.validation`).
    ui_kit::settings_section(ui, "validation", app.tr("prefs.validation"), |ui| {
        use solidcraft_engine::kernel::Validation;
        let label = |v: &Validation| match v {
            Validation::EachStep => "prefs.validation.each_step",
            Validation::EachFeature => "prefs.validation.each_feature",
            Validation::Off => "prefs.validation.off",
        };
        let labels: Vec<String> = Validation::ALL.iter().map(|v| app.tr(label(v)).to_string()).collect();
        let options: Vec<&str> = labels.iter().map(String::as_str).collect();
        let mut selected = Validation::ALL.iter().position(|v| *v == app.preferences.validation).unwrap_or(1);
        ui_kit::row(ui, app.tr("prefs.validation.label"), |ui| {
            ui_kit::segmented(ui, &mut selected, &options, ui_kit::Segments { intrinsic: true, ..Default::default() });
        });
        if let Some(v) = Validation::ALL.get(selected) {
            app.preferences.validation = *v;
        }
        ui_kit::text(ui, app.tr("prefs.validation_note"), ui_kit::Text::Hint("validation"));
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
        setting::hide_origin(app, ui);
        ui_kit::text(ui, app.tr("ui.hide_origin_note"), ui_kit::Text::Hint("hide_origin"));
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
    ui_kit::settings_section(ui, "placement", app.tr("prefs.placement"), |ui| {
        setting::placement_handles(app, ui);
    });
    // Which geometry kernel answers; `prefs::apply` applies it through diagnostics.kernel.
    ui_kit::settings_section(ui, "geometry_kernel", app.tr("prefs.geometry_kernel"), |ui| {
        // Under SOLIDCRAFT_KERNEL the control shows the environment's choice, disabled.
        let environment = app.prefs_window.kernel_environment.clone();
        ui.add_enabled_ui(environment.is_none(), |ui| {
            let options = [app.tr("prefs.geometry_kernel_routed"), "SMLib", "solidcore"];
            let shown = environment.as_deref().unwrap_or(&app.preferences.geometry_kernel);
            let mut selected = KERNELS.iter().position(|k| *k == shown).unwrap_or(0);
            ui_kit::segmented(ui, &mut selected, &options, ui_kit::Segments { intrinsic: true, ..Default::default() });
            if let Some(kernel) = KERNELS.get(selected)
                && environment.is_none()
                && app.preferences.geometry_kernel != *kernel
            {
                app.preferences.geometry_kernel = (*kernel).into();
            }
        });
        ui_kit::text(ui, app.tr("prefs.geometry_kernel_note"), ui_kit::Text::Hint("geometry_kernel"));
        if environment.is_some() {
            ui_kit::text(ui, app.tr("prefs.geometry_kernel_environment"), ui_kit::Text::Hint("geometry_kernel_environment"));
        }
    });
    // Where the files a design names may be: never a network location the user hasn't allowed.
    ui_kit::settings_section(ui, "network_locations", app.tr("prefs.network_locations_section"), |ui| {
        setting::network_locations(app, ui);
        ui_kit::text(ui, app.tr("prefs.network_locations_note"), ui_kit::Text::Hint("network_locations"));
        let mut remove = None;
        for (i, host) in app.preferences.network_hosts.iter().enumerate() {
            ui.horizontal(|ui| {
                ui_kit::text(ui, &format!("\\\\{host}"), ui_kit::Text::Hint("network_host"));
                if ui_kit::button(ui, app.tr("prefs.network_host_remove"), ui_kit::Button::Small).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove.filter(|i| *i < app.preferences.network_hosts.len()) {
            app.preferences.network_hosts.remove(i);
        }
    });
}
