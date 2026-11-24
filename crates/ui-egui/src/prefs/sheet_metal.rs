//! Sheet metal defaults: the rule new designs get, and how the flat pattern is exported as DXF.
use super::*;

/// The rules the default-rule box offers: the open design's rules, and the personal rules that
/// this version can offer and are not already among them.
fn system_rules(app: &SolveApp) -> Vec<solidcraft_engine::doc::sheet::SheetRule> {
    let mut rules = crate::dialogs_sheet::design_rules(&app.session.doc);
    for rule in app.preferences.personal_sheet_rules.rules().iter().filter(|rule| rule.supported()) {
        if !rules.iter().any(|r| r.name.eq_ignore_ascii_case(rule.name())) {
            rules.push(rule.sheet_rule());
        }
    }
    rules
}

/// The picked rule's values as literals: its lengths and K factor read in this design (where they
/// may name this design's parameters), so the default the preference stores applies to a design
/// that defines none of them. A rule this design cannot evaluate is kept as written; applying it
/// then says so instead of falling back silently.
fn rule_literals(app: &SolveApp, rule: &solidcraft_engine::doc::sheet::SheetRule) -> solidcraft_engine::doc::sheet::SheetRule {
    use solidcraft_engine::doc::sheet::SheetRule;
    let (vals, _) = app.session.doc.param_values();
    let Some(v) = app.session.doc.rule_values(&vals, rule).ok() else {
        return rule.clone();
    };
    SheetRule {
        name: rule.name.clone(),
        thickness: format!("{} mm", v.thickness),
        k_factor: v.k_factor.to_string(),
        bend_radius: format!("{} mm", v.bend_radius),
        relief_width: format!("{} mm", v.relief_width),
        relief_depth: format!("{} mm", v.relief_depth),
        corner_relief: format!("{} mm", v.corner_relief),
        corner_relief_shape: v.corner_relief_shape.name().to_string(),
        hem_gap: format!("{} mm", v.hem_gap),
        gap: format!("{} mm", v.gap),
        angle_display: rule.angle_display.clone(),
        material: rule.material.clone(),
    }
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let labels = t.space_24 * 6.0 + t.space_6;
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.structure.sheet_metal"), app.tr("prefs.structure.sheet_metal_note"));
        ui_kit::settings_section(ui, "default_rule", app.tr("prefs.sheet_metal_default_rule"), |ui| {
            ui_kit::settings_form(ui, labels, |ui| {
                let mut rules = system_rules(app);
                let before = app.preferences.sheet_rule.name.clone();
                // The stored default stays a choice even when this design has no such rule: the
                // box shows what is stored, and only a pick stores another rule.
                if !rules.iter().any(|r| r.name.eq_ignore_ascii_case(&before)) {
                    rules.push(app.preferences.sheet_rule.clone());
                }
                let names: Vec<String> = rules.iter().map(|r| r.name.clone()).collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let mut selected = rules.iter().position(|r| r.name == before).unwrap_or(0);
                ui_kit::row(ui, app.tr_label("Rule"), |ui| {
                    crate::dialogs::combo(ui, "sm_default_rule", &refs, &mut selected);
                });
                if let Some(rule) = rules.get(selected)
                    && rule.name != before
                {
                    app.preferences.sheet_rule = rule_literals(app, rule);
                    crate::prefs::default_sheet_rule_changed(app);
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
                for (setting, key, help_text_id) in [
                    (
                        setting::sheet_export_bend_lines as fn(&mut SolveApp, &mut egui::Ui) -> egui::Response,
                        "sheet_export_bend_lines",
                        "prefs.sheet_export_bend_lines_help",
                    ),
                    (setting::sheet_export_markings, "sheet_export_markings", "prefs.sheet_export_markings_help"),
                ] {
                    let text = app.tr(help_text_id);
                    ui.horizontal_wrapped(|ui| {
                        let response = setting(app, ui).backing_on_hover_text(text);
                        crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                        if !crate::ui_kit::settings_search::suppress_chrome(ui) {
                            let icon = ui_kit::help_icon(ui, text);
                            crate::scenario::publish_target(&format!("prefs:{key}_help"), icon.rect);
                        }
                    });
                }
            });
        });
    });
}
