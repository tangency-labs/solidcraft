//! Structured configuration rule editing and non-mutating row preview.
use crate::SolveApp;
use crate::ui_kit;
use egui::Ui;
use serde_json::{Value, json};
use solidcraft_engine::doc::config::{Column, ConfigRule, RuleCondition, RuleEffect, RuleOperator};

#[derive(Clone, Debug)]
pub struct Editor {
    rule: ConfigRule,
    row: String,
    preview: Option<Value>,

    preview_revision: u64,
    pub(super) new_values: std::collections::BTreeMap<String, String>,
    pub(super) adding_row: bool,
    pub(super) focus_new_row: bool,
    pub(super) drafts: std::collections::BTreeMap<(String, String), String>,
    /// The configuration table at a design revision: the command's answer and its shown cells.
    pub(super) table: Option<std::sync::Arc<(u64, Value, super::Table)>>,
    /// The Add Column menu's filter text, and whether the menu was open last frame (the filter
    /// takes focus when the menu opens and is cleared when it closes).
    pub(super) column_filter: String,
    pub(super) column_menu_open: bool,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            rule: ConfigRule {
                id: String::new(),
                name: "New rule".into(),
                enabled: true,
                length_unit: None,
                condition: RuleCondition::Always,
                effect: RuleEffect::DriveParameter { parameter: String::new(), expression: "10".into() },
            },
            row: String::new(),
            preview: None,

            preview_revision: 0,
            new_values: Default::default(),
            adding_row: false,
            focus_new_row: false,
            drafts: Default::default(),
            table: None,
            column_filter: String::new(),
            column_menu_open: false,
        }
    }
}
fn choose(ui: &mut Ui, id: &str, value: &mut String, options: &[String]) {
    let mut labels = vec![crate::i18n::tr_label("Choose…")];
    labels.extend(options.iter().map(String::as_str));
    let mut selected = options.iter().position(|o| o == value).map_or(0, |i| i + 1);
    let before = selected;
    let label = match id {
        "rule_length_units" => "Length units",
        "rule_effect_kind" => "Then",
        "condition_kind" => "When",
        "effect_parameter" | "range_parameter" | "allowed_parameter" | "condition_parameter" => "Parameter",
        "effect_feature" | "condition_feature" => "Feature",
        "preview_configuration" => "Configuration",
        _ => id,
    };
    ui_kit::row(ui, label, |ui| ui_kit::dropdown(ui, id, &mut selected, &labels));
    if before != selected
        && let Some(choice) = selected.checked_sub(1).and_then(|i| options.get(i))
    {
        *value = choice.clone();
    }
}
fn expression(ui: &mut Ui, label: &str, value: &mut String) {
    ui_kit::row(ui, label, |ui| {
        ui_kit::text_field(ui, value, label);
    });
}

pub(super) fn show(ui: &mut Ui, app: &mut SolveApp, error: &mut Option<String>, editor: &mut Editor) {
    if editor.preview_revision != app.session.revision {
        editor.preview = None;
    }
    let rules = app.session.doc.configs.rules.clone();
    let parameters: Vec<String> =
        app.session.doc.configs.columns.iter().filter_map(|c| if let Column::Param(n) = c { Some(n.clone()) } else { None }).collect();
    let all_parameters: Vec<String> = app.session.doc.all_param_exprs().into_iter().map(|p| p.0).collect();
    let features: Vec<(u64, String)> = app.session.doc.features.iter().map(|f| (f.id, f.name.clone())).collect();
    let suppression_features: Vec<(u64, String)> =
        features.iter().filter(|(id, _)| app.session.doc.configs.columns.contains(&Column::Suppress(*id))).cloned().collect();
    ui_kit::divider(ui, "configuration_rules");
    ui_kit::text(ui, crate::i18n::tr_label("Configuration rules"), ui_kit::Text::Heading("rules_heading"));
    ui_kit::text(
        ui,
        crate::i18n::tr_label("Rules run from top to bottom. Matched drives change effective values."),
        ui_kit::Text::Note("rules_drives"),
    );
    ui_kit::text(ui, crate::i18n::tr_label("Matched restrictions check the final values."), ui_kit::Text::Note("rules_restrictions"));
    for (index, rule) in rules.iter().enumerate() {
        ui.push_id(&rule.id, |ui| {
            let summary = match &rule.effect {
                RuleEffect::DriveParameter { parameter, expression } => format!("Drives {parameter} = {expression}"),
                RuleEffect::DriveSuppression { feature, value } => format!(
                    "Drives suppression {} = {value}",
                    features.iter().find(|f| f.0 == *feature).map(|f| f.1.as_str()).unwrap_or("missing feature")
                ),
                RuleEffect::ParameterRange { parameter, minimum, maximum } => format!("Restricts {parameter}: {minimum} through {maximum}"),
                RuleEffect::AllowParameterValues { parameter, expressions } => format!("Restricts {parameter}: {}", expressions.join(", ")),
            };
            let actions_width = ["Edit", "↑", "↓", "Delete"].iter().map(|label| ui_kit::ContentWidth::button(ui, label)).sum::<f32>()
                + crate::theme::Tokens::get().space_4 * 3.0;
            let mut enabled = rule.enabled;
            if ui_kit::summary_row(ui, &rule.name, &summary, &mut enabled, actions_width, |ui| {
                if ui_kit::button(ui, crate::i18n::tr_label("Edit"), ui_kit::Button::Normal).clicked() {
                    editor.rule = rule.clone();
                }
                let up = ui_kit::button(ui, "↑", ui_kit::Button::Enabled(index > 0)).on_hover_text(crate::i18n::tr_label("Move earlier"));
                crate::scenario::publish_handle(&format!("config:rule:{}:up", rule.id), up.rect.center());
                if up.clicked() {
                    *error = app.run("config.rule", json!({"id":rule.id,"index":index-1})).err();
                }
                if ui_kit::button(ui, "↓", ui_kit::Button::Enabled(index + 1 < rules.len()))
                    .on_hover_text(crate::i18n::tr_label("Move later"))
                    .clicked()
                {
                    *error = app.run("config.rule", json!({"id":rule.id,"index":index+1})).err();
                }
                if ui_kit::button(ui, crate::i18n::tr_label("Delete"), ui_kit::Button::Normal).clicked() {
                    *error = app.run("config.rule", json!({"id":rule.id,"delete":true})).err();
                }
            }) {
                let mut updated = rule.clone();
                updated.enabled = enabled;
                *error = app.run("config.rule", json!({"rule":updated})).err();
            }
        });
    }
    ui_kit::section(ui, "rule_editor", "Add or edit rule", false, |ui| {
        ui.vertical(|ui| {
            expression(ui, "Rule name", &mut editor.rule.name);
            ui_kit::checkbox(ui, crate::i18n::tr_label("Enabled"), &mut editor.rule.enabled);
            if ui_kit::button(ui, crate::i18n::tr_label("New"), ui_kit::Button::Small).clicked() {
                editor.rule = Editor::default().rule;
            }
        });
        let mut length_unit = editor.rule.length_unit.clone().unwrap_or_else(|| app.session.doc.units.clone());
        let old_unit = length_unit.clone();
        ui.vertical(|ui| {
            choose(ui, "rule_length_units", &mut length_unit, &["mm".into(), "cm".into(), "m".into(), "in".into(), "ft".into()]);
        });
        if length_unit != old_unit {
            editor.rule.length_unit = Some(length_unit);
        }
        condition_editor(ui, &mut editor.rule.condition, &all_parameters, &features, 1);
        let mut effect_kind = match editor.rule.effect {
            RuleEffect::DriveParameter { .. } => "Drive parameter",
            RuleEffect::DriveSuppression { .. } => "Drive suppression",
            RuleEffect::ParameterRange { .. } => "Range",
            RuleEffect::AllowParameterValues { .. } => "Allowed values",
        }
        .to_string();
        let prior = effect_kind.clone();
        ui.vertical(|ui| {
            choose(
                ui,
                "rule_effect_kind",
                &mut effect_kind,
                &["Drive parameter".into(), "Drive suppression".into(), "Range".into(), "Allowed values".into()],
            );
        });
        if prior != effect_kind {
            let parameter = parameters.first().cloned().unwrap_or_default();
            editor.rule.effect = match effect_kind.as_str() {
                "Drive suppression" => RuleEffect::DriveSuppression { feature: suppression_features.first().map(|f| f.0).unwrap_or(0), value: true },
                "Range" => RuleEffect::ParameterRange { parameter, minimum: "0".into(), maximum: "100".into() },
                "Allowed values" => RuleEffect::AllowParameterValues { parameter, expressions: vec!["10".into(), "20".into()] },
                _ => RuleEffect::DriveParameter { parameter, expression: "10".into() },
            };
        }
        match &mut editor.rule.effect {
            RuleEffect::DriveParameter { parameter, expression: value } => {
                ui.vertical(|ui| {
                    choose(ui, "effect_parameter", parameter, &parameters);
                    expression(ui, "Value", value);
                });
            }
            RuleEffect::DriveSuppression { feature, value } => {
                ui.vertical(|ui| {
                    feature_choice(ui, "effect_feature", feature, &suppression_features);
                    ui_kit::checkbox(ui, crate::i18n::tr_label("Suppressed"), value);
                });
            }
            RuleEffect::ParameterRange { parameter, minimum, maximum } => {
                choose(ui, "range_parameter", parameter, &parameters);
                ui.vertical(|ui| {
                    expression(ui, "Minimum", minimum);
                });
                ui.vertical(|ui| {
                    expression(ui, "Maximum", maximum);
                });
            }
            RuleEffect::AllowParameterValues { parameter, expressions } => {
                choose(ui, "allowed_parameter", parameter, &parameters);
                let mut text = expressions.join("; ");
                if ui_kit::row(ui, "Allowed expressions, separated by semicolons", |ui| ui_kit::text_field(ui, &mut text, "").1) {
                    *expressions = text.split(';').map(|s| s.trim().to_string()).collect();
                }
            }
        }
        let save = ui_kit::button(ui, "Save Rule", ui_kit::Button::Enabled(!app.session.doc.configs.rows.is_empty()));
        crate::scenario::publish_handle("config:save_rule", save.rect.center());
        if save.clicked() {
            let result = (|| -> Result<(), String> {
                let mut rule = editor.rule.clone();
                if rule.id.is_empty() {
                    rule.id = (1..).map(|n| format!("Rule{n}")).find(|id| !rules.iter().any(|r| r.id == *id)).unwrap_or_default();
                }
                app.run("config.rule", json!({"rule":rule.clone()}))?;
                editor.rule = app.session.doc.configs.rules.iter().find(|existing| existing.id == rule.id).cloned().unwrap_or(rule);
                Ok(())
            })();
            *error = result.err();
        }
    });
    ui_kit::section(ui, "preview", "Preview", true, |ui| {
        let rows: Vec<String> = app.session.doc.configs.rows.iter().map(|r| r.name.clone()).collect();
        if !rows.contains(&editor.row) {
            editor.row = app.session.doc.configs.active.clone().or_else(|| rows.first().cloned()).unwrap_or_default();
        }
        let previous_row = editor.row.clone();
        ui.vertical(|ui| {
            choose(ui, "preview_configuration", &mut editor.row, &rows);
            let preview = ui_kit::button(ui, "Preview Row", ui_kit::Button::Enabled(!editor.row.is_empty()));
            crate::scenario::publish_handle("config:preview_row", preview.rect.center());
            if preview.clicked() {
                match app.session.execute("config.preview", &json!({"row":editor.row})) {
                    Ok(result) => {
                        editor.preview = Some(result);
                        editor.preview_revision = app.session.revision;
                        *error = None;
                    }
                    Err(e) => {
                        editor.preview = None;
                        *error = Some(e.to_string());
                    }
                }
            }
        });
        if previous_row != editor.row {
            editor.preview = None;
        }
        if let Some(preview) = &editor.preview {
            ui_kit::section(ui, "rule_preview", &format!("Preview: {}", preview["active"].as_str().unwrap_or("row")), false, |ui| {
                report(ui, crate::i18n::tr_label("Preview leaves the current design unchanged."));
                if let Some(styles) = preview["styles"].as_array() {
                    for style in styles {
                        report(
                            ui,
                            format!(
                                "{}: {} (shared definition)",
                                style["target"].as_str().unwrap_or("Appearance"),
                                if style["dormant"].as_bool() == Some(true) { "dormant" } else { "resolved" }
                            ),
                        );
                    }
                }
                if let Some(trace) = preview["trace"].as_array() {
                    for entry in trace {
                        report(
                            ui,
                            format!(
                                "{} — {}",
                                entry["name"].as_str().unwrap_or("rule"),
                                if entry["matched"].as_bool() == Some(true) { "matched" } else { "skipped" }
                            ),
                        );
                        if entry["matched"].as_bool() == Some(true)
                            && let Ok(effect) = serde_json::from_value::<RuleEffect>(entry["effect"].clone())
                        {
                            report(
                                ui,
                                match effect {
                                    RuleEffect::DriveParameter { parameter, expression } => format!("Driven: {parameter} = {expression}"),
                                    RuleEffect::DriveSuppression { feature, value } => format!(
                                        "Driven: {} is {}",
                                        features.iter().find(|f| f.0 == feature).map(|f| f.1.as_str()).unwrap_or("feature"),
                                        if value { "suppressed" } else { "unsuppressed" }
                                    ),
                                    RuleEffect::ParameterRange { parameter, minimum, maximum } => {
                                        format!("Restricted: {parameter}, {minimum} through {maximum}")
                                    }
                                    RuleEffect::AllowParameterValues { parameter, expressions } => {
                                        format!("Restricted: {parameter}, {}", expressions.join("; "))
                                    }
                                },
                            );
                        }
                    }
                }
                if let Some(parameters) = preview["parameters"].as_array() {
                    for parameter in parameters {
                        if let Some(name) = parameter["name"].as_str()
                            && all_parameters.iter().any(|p| p == name)
                        {
                            let response = ui_kit::text(
                                ui,
                                &format!("{name} = {}", parameter["expression"].as_str().unwrap_or("")),
                                ui_kit::Text::Report("preview_parameter", false),
                            );
                            crate::scenario::publish_handle(&format!("config:preview:param:{name}"), response.rect.center());
                        }
                    }
                }
            });
        }
    });
}
fn report(ui: &mut Ui, text: impl AsRef<str>) {
    ui_kit::text(ui, crate::i18n::tr_label(text.as_ref()), ui_kit::Text::Report("rule_report", false));
}

fn feature_choice(ui: &mut Ui, id: &str, feature: &mut u64, features: &[(u64, String)]) {
    let options: Vec<_> = features.iter().map(|(_, name)| name.clone()).collect();
    let mut selected = features.iter().find(|f| f.0 == *feature).map(|f| f.1.clone()).unwrap_or_default();
    choose(ui, id, &mut selected, &options);
    if let Some((id, _)) = features.iter().find(|(_, name)| *name == selected) {
        *feature = *id;
    }
}

fn condition_editor(ui: &mut Ui, condition: &mut RuleCondition, parameters: &[String], features: &[(u64, String)], depth: usize) {
    let mut kind = match condition {
        RuleCondition::Always => "Always",
        RuleCondition::Parameter { .. } => "Parameter",
        RuleCondition::Suppression { .. } => "Suppression",
        RuleCondition::All { .. } => "All",
        RuleCondition::Any { .. } => "Any",
        RuleCondition::Not { .. } => "Not",
    }
    .to_string();
    let previous = kind.clone();
    let mut choices = vec!["Always".into(), "Parameter".into(), "Suppression".into()];
    if depth < 8 {
        choices.extend(["All".into(), "Any".into(), "Not".into()]);
    }
    ui.vertical(|ui| {
        choose(ui, "condition_kind", &mut kind, &choices);
    });
    if previous != kind {
        *condition = match kind.as_str() {
            "Parameter" => RuleCondition::Parameter {
                parameter: parameters.first().cloned().unwrap_or_default(),
                operator: RuleOperator::Ge,
                expression: "10".into(),
            },
            "Suppression" => RuleCondition::Suppression { feature: features.first().map(|f| f.0).unwrap_or(0), value: true },
            "All" => RuleCondition::All { conditions: vec![RuleCondition::Always] },
            "Any" => RuleCondition::Any { conditions: vec![RuleCondition::Always] },
            "Not" => RuleCondition::Not { condition: Box::new(RuleCondition::Always) },
            _ => RuleCondition::Always,
        };
    }
    match condition {
        RuleCondition::Parameter { parameter, operator, expression } => {
            ui.vertical(|ui| {
                choose(ui, "condition_parameter", parameter, parameters);
                let operators = [
                    ("=", RuleOperator::Eq),
                    ("≠", RuleOperator::Ne),
                    ("<", RuleOperator::Lt),
                    ("≤", RuleOperator::Le),
                    (">", RuleOperator::Gt),
                    ("≥", RuleOperator::Ge),
                ];
                let mut index = operators.iter().position(|(_, op)| op == operator).unwrap_or(0);
                let labels: Vec<_> = operators.iter().map(|(label, _)| *label).collect();
                ui_kit::row(ui, "Operator", |ui| ui_kit::dropdown(ui, "condition_operator", &mut index, &labels));
                if let Some((_, op)) = operators.get(index) {
                    *operator = *op;
                }
                ui_kit::row(ui, "Expression", |ui| ui_kit::field(ui, expression));
            });
        }
        RuleCondition::Suppression { feature, value } => {
            ui.vertical(|ui| {
                feature_choice(ui, "condition_feature", feature, features);
                ui_kit::checkbox(ui, crate::i18n::tr_label("Suppressed"), value);
            });
        }
        RuleCondition::All { conditions } | RuleCondition::Any { conditions } => {
            let mut remove = None;
            for (index, child) in conditions.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.indent("condition_group", |ui| {
                        condition_editor(ui, child, parameters, features, depth + 1);
                        if ui_kit::button(ui, crate::i18n::tr_label("Remove condition"), ui_kit::Button::Small).clicked() {
                            remove = Some(index);
                        }
                    });
                });
            }
            if let Some(index) = remove {
                conditions.remove(index);
            }
            if ui_kit::button(ui, "Add condition", ui_kit::Button::Enabled(conditions.len() < 32)).clicked() {
                conditions.push(RuleCondition::Always);
            }
        }
        RuleCondition::Not { condition } => {
            ui.indent("negated_condition", |ui| {
                condition_editor(ui, condition, parameters, features, depth + 1);
            });
        }
        RuleCondition::Always => {}
    }
}
