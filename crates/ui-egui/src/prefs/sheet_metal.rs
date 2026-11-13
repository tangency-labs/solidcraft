//! Sheet metal defaults: the rule new designs get, and how the flat pattern is exported as DXF.
use super::*;

/// Which of this page's help notes a click on the (i) icon holds open.
#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct HelpOpen {
    pub(crate) bend_lines: bool,
    pub(crate) markings: bool,
}

fn help_id() -> egui::Id {
    egui::Id::unique("sheet_metal_prefs_help")
}

fn help_state(ui: &egui::Ui) -> HelpOpen {
    ui.ctx().data(|data| data.get_temp::<HelpOpen>(help_id())).unwrap_or_default()
}

/// The rules the default-rule box offers: the open design's rules, and the personal rules that
/// are not already among them.
fn system_rules(app: &SolveApp) -> Vec<solidcraft_engine::doc::sheet::SheetRule> {
    let mut rules = crate::dialogs_sheet::design_rules(&app.session.doc);
    for rule in app.preferences.personal_sheet_rules.rules() {
        if !rules.iter().any(|r| r.name.eq_ignore_ascii_case(rule.name())) {
            rules.push(rule.sheet_rule());
        }
    }
    rules
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let labels = t.space_24 * 6.0 + t.space_6;
    let mut help = help_state(ui);
    ui_kit::settings_page(ui, |ui| {
        ui_kit::settings_heading(ui, app.tr("prefs.structure.sheet_metal"), app.tr("prefs.structure.sheet_metal_note"));
        ui_kit::settings_section(ui, "default_rule", app.tr("prefs.sheet_metal_default_rule"), |ui| {
            ui_kit::settings_form(ui, labels, |ui| {
                let rules = system_rules(app);
                let names: Vec<String> = rules.iter().map(|r| r.name.clone()).collect();
                let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                let before = app.preferences.sheet_rule.name.clone();
                let mut selected = rules.iter().position(|r| r.name == before).unwrap_or(0);
                ui_kit::row(ui, app.tr_label("Rule"), |ui| {
                    crate::dialogs::combo(ui, "sm_default_rule", &refs, &mut selected);
                });
                if let Some(rule) = rules.get(selected)
                    && rule.name != before
                {
                    app.preferences.sheet_rule = rule.clone();
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
                let mut help_changed = false;
                for (setting, key, help_text_id, open) in [
                    (
                        setting::sheet_export_bend_lines as fn(&mut SolveApp, &mut egui::Ui) -> egui::Response,
                        "sheet_export_bend_lines",
                        "prefs.sheet_export_bend_lines_help",
                        &mut help.bend_lines,
                    ),
                    (setting::sheet_export_markings, "sheet_export_markings", "prefs.sheet_export_markings_help", &mut help.markings),
                ] {
                    let text = app.tr(help_text_id);
                    ui.horizontal_wrapped(|ui| {
                        let response = setting(app, ui).backing_on_hover_text(text);
                        crate::scenario::publish_target(&format!("prefs:{key}"), response.rect);
                        if !crate::ui_kit::settings_search::suppress_chrome(ui) {
                            let icon = ui_kit::help_icon(ui, text);
                            crate::scenario::publish_target(&format!("prefs:{key}_help"), icon.rect);
                            if icon.clicked() {
                                *open = !*open;
                                help_changed = true;
                            }
                        }
                    });
                    if *open {
                        ui_kit::settings_inline_note(ui, text);
                    }
                }
                if help_changed {
                    ui.ctx().data_mut(|data| data.insert_temp::<HelpOpen>(help_id(), help));
                }
            });
        });
    });
}
