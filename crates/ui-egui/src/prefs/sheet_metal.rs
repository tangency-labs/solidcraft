//! Sheet metal defaults: the rule new designs get, and how the flat pattern is exported as DXF.
use super::*;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let labels = t.space_24 * 6.0 + t.space_6;
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.structure.sheet_metal"), app.tr("prefs.structure.sheet_metal_note"));
        ui_kit::settings_section(ui, "default_rule", app.tr("prefs.sheet_metal_default_rule"), |ui| {
            ui_kit::settings_form(ui, labels, |ui| {
                let fields = ["Thickness", "K Factor", "Bend Radius"];
                for (i, field_label) in fields.iter().enumerate() {
                    let mut changed = false;
                    ui_kit::row(ui, app.tr_label(field_label), |ui| {
                        let field = match i {
                            0 => &mut app.preferences.sheet_rule.thickness,
                            1 => &mut app.preferences.sheet_rule.k_factor,
                            _ => &mut app.preferences.sheet_rule.bend_radius,
                        };
                        changed = ui_kit::field(ui, field);
                    });
                    if changed {
                        crate::prefs::default_sheet_rule_changed(app);
                    }
                }
            });
        });
        ui_kit::settings_section(ui, "dxf_export", app.tr("prefs.sheet_metal_dxf_export"), |ui| {
            ui_kit::settings_form(ui, labels, |ui| {
                ui_kit::row(ui, app.tr("prefs.sheet_dxf_units"), |ui| {
                    let units = ["mm", "inches"];
                    let mut selected = (app.preferences.sheet_dxf_units == "inches") as usize;
                    ui_kit::segmented(
                        ui,
                        &mut selected,
                        &["MM", "IN"],
                        ui_kit::Segments {
                            intrinsic: true,
                            targets: &["prefs:sheet_dxf_units:mm", "prefs:sheet_dxf_units:inches"],
                            ..Default::default()
                        },
                    );
                    if let Some(unit) = units.get(selected) {
                        app.preferences.sheet_dxf_units = (*unit).into();
                    }
                });
                for (setting, help, key) in [
                    (
                        setting::sheet_export_bend_lines as fn(&mut SolveApp, &mut egui::Ui) -> egui::Response,
                        "prefs.sheet_export_bend_lines_help",
                        "sheet_export_bend_lines",
                    ),
                    (setting::sheet_export_markings, "prefs.sheet_export_markings_help", "sheet_export_markings"),
                ] {
                    ui.horizontal_wrapped(|ui| {
                        let response = setting(app, ui).backing_on_hover_text(app.tr(help));
                        crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                        if !crate::ui_kit::settings_search::suppress_chrome(ui) {
                            ui_kit::help_icon(ui, app.tr(help));
                        }
                    });
                }
            });
        });
    });
}
