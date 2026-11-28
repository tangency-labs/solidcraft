//! Structured configuration rule editing and non-mutating row preview.
use crate::SolveApp;
use crate::ui_kit;
use crate::ui_kit::BackingTooltipExt as _;
use egui::Ui;
use serde_json::{Value, json};
use solidcraft_engine::doc::config::{Column, ConfigRule, RuleCondition, RuleEffect, RuleOperator};

#[derive(Clone, Debug)]
pub struct Editor {
    rule: ConfigRule,
    row: String,
    preview: Option<Value>,

    preview_revision: u64,
    preview_bands: [bool; 3],
    pub(super) new_values: std::collections::BTreeMap<String, String>,
    pub(super) adding_row: bool,
    pub(super) focus_new_row: bool,
    /// The row being renamed (its name, the text typed so far), opened by a double-click on its name.
    pub(super) renaming: Option<(String, String)>,
    pub(super) drafts: std::collections::BTreeMap<(String, String), String>,
    /// The configuration table at a design revision: the command's answer and its shown cells.
    pub(super) table: Option<std::sync::Arc<(u64, Value, super::Table)>>,
    /// The Add Column menu's filter text, and whether the menu was open last frame (the filter
    /// takes focus when the menu opens and is cleared when it closes).
    pub(super) column_filter: String,
    pub(super) column_menu_open: bool,
    /// After Update: the changes the table has no column for (`config.update_from_design`'s
    /// `not_captured`), at the design revision the update made.
    pub(super) not_captured: Option<(u64, Vec<String>)>,
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
            preview_bands: [true; 3],
            new_values: Default::default(),
            adding_row: false,
            focus_new_row: false,
            renaming: None,
            drafts: Default::default(),
            table: None,
            column_filter: String::new(),
            column_menu_open: false,
            not_captured: None,
        }
    }
}
fn choose(ui: &mut Ui, id: &str, value: &mut String, options: &[String]) {
    let mut labels = vec![solidcraft_ui_text::tr_label("Choose…")];
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
    let response = ui_kit::row(ui, label, |ui| ui_kit::dropdown(ui, id, &mut selected, &labels));
    crate::scenario::publish_handle(&format!("config:choice:{id}"), response.rect.center());
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
    // A rule's target must be a column of the table (the engine refuses any other), so the Then
    // lists leave out what has none; a note then says so, where When lists everything (#10532).
    let unlisted_features = suppression_features.len() < features.len();
    let unlisted_parameters = all_parameters.iter().any(|p| !parameters.contains(p));
    ui_kit::divider(ui, "configuration_rules");
    ui_kit::text(ui, solidcraft_ui_text::tr_label("Configuration rules"), ui_kit::Text::Heading("rules_heading"));
    ui_kit::text(
        ui,
        solidcraft_ui_text::tr_label("Rules run from top to bottom. Matched drives change effective values."),
        ui_kit::Text::Note("rules_drives"),
    );
    ui_kit::text(ui, solidcraft_ui_text::tr_label("Matched restrictions check the final values."), ui_kit::Text::Note("rules_restrictions"));
    let t = solidcraft_theme::Tokens::get();
    let columns = [
        ui_kit::Column::new("Enabled", 0.0).min_content_width(t.form_close_size),
        ui_kit::Column::new("Rule name", 0.8).flex().min_content_width(t.form_label_width),
        ui_kit::Column::new("Then", 1.4).flex().min_content_width(t.form_label_width),
        ui_kit::Column::new("Actions", 0.0)
            .icon_only()
            .min_content_width(ui_kit::ContentWidth::button(ui, "Edit") + 3.0 * t.control_height + 3.0 * ui.spacing().item_spacing.x),
    ];
    ui_kit::table_custom(ui, "configuration_rules", &columns, solidcraft_ui_text::tr("manage.empty"), |ui, table| {
        table.headers(ui, |_, _| false);
        for (index, rule) in rules.iter().enumerate() {
            let mut enabled = rule.enabled;
            table.row(ui, &rule.id, false, |row| {
                row.cell(|ui| {
                    let response = ui_kit::checkbox(ui, "", &mut enabled).backing_on_hover_text(solidcraft_ui_text::tr_label("Enable rule"));
                    crate::scenario::publish_handle(&format!("config:rule:{}:enabled", rule.id), response.rect.center());
                });
                row.cell(|ui| ui_kit::text(ui, &rule.name, ui_kit::Text::NameCell(true)));
                row.cell(|ui| ui_kit::text(ui, &effect_summary(&rule.effect, &features), ui_kit::Text::Cell));
                row.cell(|ui| {
                    if ui_kit::button(ui, solidcraft_ui_text::tr_label("Edit"), ui_kit::Button::Normal).clicked() {
                        editor.rule = rule.clone();
                    }
                    for (glyph, tip, side, target) in [
                        (ui_kit::MOVE_UP, "Move earlier", "up", index.checked_sub(1)),
                        (ui_kit::MOVE_DOWN, "Move later", "down", (index + 1 < rules.len()).then_some(index + 1)),
                    ] {
                        let response = ui_kit::compact_button(ui, glyph, tip, target.is_some(), false);
                        crate::scenario::publish_handle(&format!("config:rule:{}:{side}", rule.id), response.rect.center());
                        if response.clicked()
                            && let Some(target) = target
                        {
                            *error = app.run("config.rule", json!({"id":rule.id,"index":target})).err();
                        }
                    }
                    if ui_kit::bin_button(ui, "Delete").clicked() {
                        *error = app.run("config.rule", json!({"id":rule.id,"delete":true})).err();
                    }
                });
            });
            if enabled != rule.enabled {
                let mut updated = rule.clone();
                updated.enabled = enabled;
                *error = app.run("config.rule", json!({"rule":updated})).err();
            }
        }
    });
    ui_kit::section(ui, "rule_editor", "Add or edit rule", false, |ui| {
        ui.vertical(|ui| {
            expression(ui, "Rule name", &mut editor.rule.name);
            ui_kit::checkbox(ui, solidcraft_ui_text::tr_label("Enabled"), &mut editor.rule.enabled);
            if ui_kit::button(ui, solidcraft_ui_text::tr_label("New"), ui_kit::Button::Small).clicked() {
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
                    columns_note(ui, unlisted_parameters, "config.rule_parameter_columns");
                    expression(ui, "Value", value);
                });
            }
            RuleEffect::DriveSuppression { feature, value } => {
                ui.vertical(|ui| {
                    feature_choice(ui, "effect_feature", feature, &suppression_features);
                    columns_note(ui, unlisted_features, "config.rule_suppression_columns");
                    ui_kit::checkbox(ui, solidcraft_ui_text::tr_label("Suppressed"), value);
                });
            }
            RuleEffect::ParameterRange { parameter, minimum, maximum } => {
                choose(ui, "range_parameter", parameter, &parameters);
                columns_note(ui, unlisted_parameters, "config.rule_parameter_columns");
                ui.vertical(|ui| {
                    expression(ui, "Minimum", minimum);
                });
                ui.vertical(|ui| {
                    expression(ui, "Maximum", maximum);
                });
            }
            RuleEffect::AllowParameterValues { parameter, expressions } => {
                choose(ui, "allowed_parameter", parameter, &parameters);
                columns_note(ui, unlisted_parameters, "config.rule_parameter_columns");
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
            let title = solidcraft_ui_text::format("config.rules.preview", &[("row", preview["active"].as_str().unwrap_or_default())]);
            ui_kit::section(ui, "rule_preview", &title, false, |ui| {
                ui_kit::text(ui, solidcraft_ui_text::tr_label("Preview leaves the current design unchanged."), ui_kit::Text::Note("rule_report"));
                let columns = [
                    ui_kit::Column::new("Name", 1.0).flex().min_content_width(t.form_label_width),
                    ui_kit::Column::new("Status", 0.6).flex().min_content_width(t.form_close_size * 3.0),
                    ui_kit::Column::new("Result", 1.4).flex().min_content_width(t.form_label_width),
                ];
                ui_kit::table_custom(ui, "configuration_preview", &columns, solidcraft_ui_text::tr("manage.empty"), |ui, table| {
                    table.headers(ui, |_, _| false);
                    for ((key, label), open) in [
                        ("styles", solidcraft_ui_text::tr("render.appearances")),
                        ("trace", solidcraft_ui_text::tr_label("Rules")),
                        ("parameters", solidcraft_ui_text::tr("ui.parameters")),
                    ]
                    .into_iter()
                    .zip(&mut editor.preview_bands)
                    {
                        let entries: Vec<_> = preview[key]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|entry| {
                                key != "parameters" || entry["name"].as_str().is_some_and(|name| all_parameters.iter().any(|p| p == name))
                            })
                            .collect();
                        if entries.is_empty() {
                            continue;
                        }
                        table.group(ui, key, label, entries.len(), open);
                        if !*open {
                            continue;
                        }
                        for (index, entry) in entries.into_iter().enumerate() {
                            let (name, status, result) = match key {
                                "styles" => (
                                    entry["target"].as_str().unwrap_or_else(|| solidcraft_ui_text::tr_label("Appearance")),
                                    solidcraft_ui_text::tr(if entry["dormant"].as_bool() == Some(true) {
                                        "config.rules.dormant"
                                    } else {
                                        "config.rules.resolved"
                                    }),
                                    solidcraft_ui_text::tr("config.rules.shared_definition").to_owned(),
                                ),
                                "trace" => {
                                    let matched = entry["matched"].as_bool() == Some(true);
                                    let result = if matched {
                                        serde_json::from_value::<RuleEffect>(entry["effect"].clone())
                                            .ok()
                                            .map(|effect| effect_summary(&effect, &features))
                                            .unwrap_or_default()
                                    } else {
                                        String::new()
                                    };
                                    (
                                        entry["name"].as_str().unwrap_or_else(|| solidcraft_ui_text::tr_label("Rule")),
                                        solidcraft_ui_text::tr(if matched { "config.rules.matched" } else { "config.rules.skipped" }),
                                        result,
                                    )
                                }
                                _ => (entry["name"].as_str().unwrap_or_default(), "", entry["expression"].as_str().unwrap_or_default().to_owned()),
                            };
                            table.row(ui, &format!("{key}:{index}"), false, |row| {
                                row.cell(|ui| ui_kit::text(ui, name, ui_kit::Text::NameCell(false)));
                                row.cell(|ui| ui_kit::text(ui, status, ui_kit::Text::Cell));
                                row.cell(|ui| {
                                    let response =
                                        ui_kit::text(ui, &result, if key == "parameters" { ui_kit::Text::MonoCell } else { ui_kit::Text::Cell });
                                    if key == "parameters" {
                                        crate::scenario::publish_handle(&format!("config:preview:param:{name}"), response.rect.center());
                                    }
                                });
                            });
                        }
                    }
                });
            });
        }
    });
}
/// Under a Then list that leaves some out (`unlisted`): only the table's columns are listed, and
/// how to add one.
fn columns_note(ui: &mut Ui, unlisted: bool, message: &'static str) {
    if unlisted {
        ui_kit::text(ui, solidcraft_ui_text::tr(message), ui_kit::Text::Note(message));
    }
}
fn effect_summary(effect: &RuleEffect, features: &[(u64, String)]) -> String {
    use solidcraft_ui_text::format;
    match effect {
        RuleEffect::DriveParameter { parameter, expression } => format("config.rules.drive", &[("parameter", parameter), ("expression", expression)]),
        RuleEffect::DriveSuppression { feature, value } => format(
            "config.rules.suppression",
            &[
                (
                    "feature",
                    features.iter().find(|f| f.0 == *feature).map(|f| f.1.as_str()).unwrap_or_else(|| solidcraft_ui_text::tr_label("Feature")),
                ),
                ("value", &value.to_string()),
            ],
        ),
        RuleEffect::ParameterRange { parameter, minimum, maximum } => {
            format("config.rules.range", &[("parameter", parameter), ("minimum", minimum), ("maximum", maximum)])
        }
        RuleEffect::AllowParameterValues { parameter, expressions } => {
            format("config.rules.allowed", &[("parameter", parameter), ("values", &expressions.join(", "))])
        }
    }
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
                ui_kit::checkbox(ui, solidcraft_ui_text::tr_label("Suppressed"), value);
            });
        }
        RuleCondition::All { conditions } | RuleCondition::Any { conditions } => {
            let mut remove = None;
            for (index, child) in conditions.iter_mut().enumerate() {
                ui.push_id(index, |ui| {
                    ui.indent("condition_group", |ui| {
                        condition_editor(ui, child, parameters, features, depth + 1);
                        if ui_kit::button(ui, solidcraft_ui_text::tr_label("Remove condition"), ui_kit::Button::Small).clicked() {
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
