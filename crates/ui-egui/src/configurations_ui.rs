//! Manage configuration table. Commands and cell decoding remain in dialogs_motion.
use super::{cell_value, column_parts, configuration_rules_ui, table, unit_of};
use crate::{SolveApp, ui_kit};
use serde_json::{Value, json};
use solidcraft_engine::doc::DEFAULT_MATERIAL;
/// The Configurations table, its rules and errors.
pub(super) fn configurations_rows(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    new_row: &mut String,
    error: &mut Option<String>,
    rule_editor: &mut configuration_rules_ui::Editor,
) {
    // Physical materials for the configuration table's Library: the library's, then the design's own.
    let material_names = app.session.doc.material_names();
    // The table and its cells are made once per design revision: a table of hundreds of rows
    // costs too much to rebuild every frame.
    let revision = app.session.revision;
    if rule_editor.table.as_ref().is_none_or(|shown| shown.0 != revision) {
        let value = app.session.execute("config.table", &json!({})).unwrap_or(Value::Null);
        let tb = with_units(app, table(&value));
        rule_editor.table = Some(std::sync::Arc::new((revision, value, tb)));
    }
    let Some(shown) = rule_editor.table.clone() else { return };
    let (_, v, tb) = &*shown;
    let run = |app: &mut SolveApp, error: &mut Option<String>, id: &str, p: Value| {
        *error = app.run(id, p).err();
    };
    let labels: Vec<_> = std::iter::once(crate::i18n::tr_label("Configuration").to_owned())
        .chain(tb.columns.iter().map(|c| {
            let (kind, name) = column_parts(c);
            v["targets"]
                .as_array()
                .and_then(|targets| targets.iter().find(|target| target["key"].as_str() == Some(c)))
                .and_then(|target| target["label"].as_str())
                .filter(|_| matches!(kind, "component_appearance" | "face_appearance"))
                .map(str::to_string)
                .unwrap_or_else(|| match kind {
                    "material" => format!("{}: {name}", crate::i18n::tr_label("Material")),
                    "appearance" => format!("{}: {name}", crate::i18n::tr_label("Appearance")),
                    "suppress" => format!("{}: {name}", crate::i18n::tr_label("Suppress")),
                    _ => name.to_owned(),
                })
        }))
        .chain(std::iter::once(crate::i18n::tr_label("Actions").to_owned()))
        .collect();
    let gap = ui.spacing().item_spacing.x;
    let name_width = tb.rows.iter().map(|(name, _)| ui_kit::ContentWidth::name(ui, name)).fold(0.0, f32::max);
    let status_width = (ui_kit::ContentWidth::label(ui, "Active") + gap + ui_kit::ContentWidth::button(ui, "Apply"))
        .max(ui_kit::ContentWidth::button(ui, "Activate"));
    let columns: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let min_width = if index == 0 {
                name_width + gap + status_width + crate::theme::Tokens::get().space_24
            } else {
                ui_kit::row_height() * 2.0 + crate::theme::Tokens::get().space_24
            };
            if index == labels.len() - 1 {
                ui_kit::Column::new(label, 0.0)
                    .icon_only()
                    .min_content_width(crate::theme::Tokens::get().control_height + crate::theme::Tokens::get().space_12)
            } else {
                let weight = if index == 0 {
                    1.6
                } else {
                    match tb.columns.get(index - 1).map(|column| column_parts(column).0) {
                        Some("param") => 0.9,
                        Some("suppress") => 1.1,
                        Some("material") => 1.2,
                        _ => 1.3,
                    }
                };
                ui_kit::Column::new(label, weight).flex().min_content_width(min_width)
            }
        })
        .collect();
    ui_kit::table_custom(ui, "configuration_table", &columns, crate::i18n::tr("manage.empty"), |ui, table| {
        table.headers(ui, |ui, index| {
            if index == labels.len() - 1 {
                column_menu(app, ui, error, &tb.columns, rule_editor);
                true
            } else if index > 0 {
                if let (Some(column), Some(label)) = (tb.columns.get(index - 1), labels.get(index))
                    && ui_kit::removable_header(ui, label)
                {
                    let (kind, name) = column_parts(column);
                    let key = if kind == "suppress" { "feature" } else { kind };
                    run(app, error, "config.column", json!({key:if kind == "face_appearance" {column.as_str()} else {name},"delete":true}));
                }
                true
            } else {
                false
            }
        });
        for (name, values) in &tb.rows {
            let active = tb.active.as_deref() == Some(name.as_str());
            // Rows off screen keep their space without building their cells; one with an edit
            // in progress stays live, so leaving its cell still commits the edit.
            let editing = rule_editor.drafts.keys().any(|(row, _)| row == name);
            table.row_visible(ui, name, active, editing, |row| {
                row.cell(|ui| {
                    // A measured name slot keeps status/actions inside the Configuration column.
                    ui.allocate_ui_with_layout(
                        egui::vec2(name_width, ui_kit::row_height()),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui_kit::text(ui, name, ui_kit::Text::NameCell(true));
                        },
                    );
                    if active {
                        ui_kit::active_pill(ui);
                        if v.get("matches").and_then(Value::as_bool) == Some(false) {
                            let apply = ui_kit::button(ui, crate::i18n::tr_label("Apply"), ui_kit::Button::Small);
                            crate::scenario::publish_handle(&format!("config:activate:{name}"), apply.rect.center());
                            if apply.clicked() {
                                run(app, error, "config.activate", json!({"row":name}));
                            }
                        }
                    } else {
                        let activate = ui_kit::button(ui, crate::i18n::tr_label("Activate"), ui_kit::Button::Small);
                        crate::scenario::publish_handle(&format!("config:activate:{name}"), activate.rect.center());
                        if activate.clicked() {
                            run(app, error, "config.activate", json!({"row":name}));
                        }
                    }
                });
                for (column, value) in tb.columns.iter().zip(values) {
                    row.cell(|ui| {
                        let (kind, _) = column_parts(column);
                        let shown = display_cell(kind, value);
                        let key = (name.clone(), column.clone());
                        let mut text = rule_editor.drafts.get(&key).cloned().unwrap_or_else(|| shown.clone());
                        let id = ui.make_persistent_id(&key);
                        let (response, choice) = value_control(ui, id, &mut text, kind, &material_names);
                        if let Some(choice) = choice {
                            run(app, error, "config.row", json!({"name":name,"values":{column:choice}}));
                        }
                        crate::scenario::publish_handle(&format!("config:cell:{name}:{column}"), response.rect.center());
                        let focused = response.has_focus();
                        let completed = crate::params_dialog::complete(ui, &response, &mut text);
                        if response.changed() || completed {
                            rule_editor.drafts.insert(key.clone(), text.clone());
                        }
                        let enter = !completed && focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                        if response.lost_focus() || enter {
                            rule_editor.drafts.remove(&key);
                            if text != shown {
                                run(app, error, "config.row", json!({"name":name,"values":{column:cell_value(column,&text)}}));
                            }
                        }
                    });
                }
                row.cell(|ui| {
                    if ui_kit::bin_button(ui, "Delete the row").clicked() {
                        run(app, error, "config.row", json!({"name":name,"delete":true}));
                    }
                });
            });
        }
        if rule_editor.adding_row {
            let menu_was_open = egui::Popup::is_any_open(ui.ctx());
            let mut commit = false;
            let mut discard = false;
            let mut left = false;
            let reveal = rule_editor.focus_new_row;
            let response = ui
                .push_id("configuration_draft", |ui| {
                    table.row(ui, "new", false, |row| {
                        row.cell(|ui| {
                            let id = ui.make_persistent_id("new_configuration");
                            let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                            let response = ui_kit::cell_editor(ui, id, new_row, "new configuration", false);
                            crate::scenario::publish_handle("config:new:name", response.rect.center());
                            if rule_editor.focus_new_row {
                                response.request_focus();
                                rule_editor.focus_new_row = false;
                            }
                            commit = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                            left = response.lost_focus();
                        });
                        let source = tb.rows.iter().find(|(name, _)| tb.active.as_ref() == Some(name));
                        for (index, column) in tb.columns.iter().enumerate() {
                            row.cell(|ui| {
                                let mut value = rule_editor.new_values.get(column).cloned().unwrap_or_else(|| {
                                    source
                                        .and_then(|(_, values)| values.get(index))
                                        .map(|value| display_cell(column_parts(column).0, value))
                                        .unwrap_or_default()
                                });
                                let (kind, _) = column_parts(column);
                                let (response, choice) = value_control(ui, ui.scope_id().with(column), &mut value, kind, &material_names);
                                let picked = choice.is_some();
                                if let Some(choice) = choice {
                                    value = choice;
                                }
                                crate::scenario::publish_handle(&format!("config:new:cell:{column}"), response.rect.center());
                                let completed = crate::params_dialog::complete(ui, &response, &mut value);
                                if response.changed() || completed || picked {
                                    rule_editor.new_values.insert(column.clone(), value);
                                }
                                let focused = response.has_focus() || response.lost_focus();
                                left |= response.lost_focus();
                                commit |= !completed && focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                            });
                        }
                        row.cell(|ui| {
                            discard = ui_kit::bin_button(ui, "Delete the row").clicked();
                        });
                    })
                })
                .inner;
            if reveal {
                table.reveal_row(response.rect);
            }
            let abandoned = new_row.trim().is_empty()
                && !menu_was_open
                && ui.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !response.rect.contains(p)));
            if discard || abandoned {
                rule_editor.adding_row = false;
                new_row.clear();
                rule_editor.new_values.clear();
            } else if commit || (left && !new_row.trim().is_empty()) {
                let name = if new_row.trim().is_empty() {
                    (tb.rows.len() + 1..).map(|k| format!("Configuration{k}")).find(|n| !tb.rows.iter().any(|r| &r.0 == n)).unwrap_or_default()
                } else {
                    new_row.trim().to_string()
                };
                let values: serde_json::Map<String, Value> =
                    rule_editor.new_values.iter().map(|(col, text)| (col.clone(), cell_value(col, text))).collect();
                let mut params = json!({"name":name});
                if !values.is_empty() {
                    params["values"] = Value::Object(values);
                }
                if let Some(active) = &tb.active {
                    params["from"] = json!(active);
                }
                run(app, error, "config.row", params);
                new_row.clear();
                rule_editor.adding_row = false;
                rule_editor.new_values.clear();
            }
        }
        if table.add_prompt(ui, "Add configuration", "A new configuration, a copy of the active one").clicked() {
            rule_editor.adding_row = true;
            rule_editor.focus_new_row = true;
        }
    });
    configuration_rules_ui::show(ui, app, error, rule_editor);
    if let Some(styles) = v["style_status"].as_array() {
        for style in styles.iter().filter(|style| style["dormant"].as_bool() == Some(true)) {
            ui_kit::text(
                ui,
                &format!("{}: appearance dormant while its feature is suppressed", style["target"].as_str().unwrap_or("Face")),
                ui_kit::Text::Report("dormant", false),
            );
        }
    }
    if let Some(message) = v["style_error"].as_str() {
        ui_kit::text(ui, message, ui_kit::Text::Error("style_error"));
    }
    if let Some(message) = error {
        ui_kit::text(ui, message, ui_kit::Text::Error("config_error"));
    }
    // Table cells and rule editors own Enter. As in the legacy wide manager, it
    // commits an edited cell without also submitting the enclosing card.
    ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
}

/// Bare numbers show with their parameter's unit ("40" is 40 mm), as Fusion shows them. The
/// unit is looked up once per column, not once per cell.
fn with_units(app: &SolveApp, mut tb: super::Table) -> super::Table {
    let kinds = app.session.doc.all_param_exprs();
    let units: Vec<Option<String>> =
        tb.columns
            .iter()
            .map(|c| {
                let (kind, n) = column_parts(c);
                if kind != "param" {
                    return None;
                }
                app.session.doc.param(n).map(|parameter| parameter.unit.clone()).or_else(|| {
                    kinds.iter().find(|x| x.0 == n).and_then(|x| {
                        unit_of(x.2).map(|unit| {
                            if x.2 == solidcraft_engine::doc::expr::Kind::Length { app.session.doc.units.clone() } else { unit.to_string() }
                        })
                    })
                })
            })
            .collect();
    for (_, vals) in &mut tb.rows {
        for (unit, val) in units.iter().zip(vals.iter_mut()) {
            if let Some(unit) = unit
                && val.trim().parse::<f64>().is_ok()
            {
                *val = format!("{} {unit}", val.trim());
            }
        }
    }
    tb
}

fn column_menu(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    error: &mut Option<String>,
    columns: &[String],
    rule_editor: &mut configuration_rules_ui::Editor,
) {
    let run = |app: &mut SolveApp, error: &mut Option<String>, id: &str, params: Value| {
        *error = app.run(id, params).err();
    };
    // A column: a parameter's value or a feature's suppression.
    let mut options: Vec<(String, Value)> = Vec::new();
    for p in app.session.doc.all_param_exprs() {
        if !columns.contains(&format!("param:{}", p.0)) {
            options.push((format!("{} (parameter)", p.0), json!({ "param": p.0 })));
        }
    }
    for f in &app.session.doc.features {
        if !columns.contains(&format!("suppress:{}", f.name)) {
            options.push((format!("Suppress {}", f.name), json!({ "feature": f.name })));
        }
    }
    for body in &app.session.model.state().bodies {
        for kind in ["material", "appearance"] {
            if !columns.contains(&format!("{kind}:{}", body.name)) {
                options.push((format!("{} ({kind})", body.name), json!({kind: body.name})));
            }
        }
    }
    for component in &app.session.doc.components {
        if component.id != 0 && !columns.contains(&format!("component_appearance:{}", component.id)) {
            options.push((format!("{} (component appearance)", component.name), json!({"component_appearance":component.id})));
        }
    }
    for selected in &app.session.selection {
        if let solidcraft_engine::Sel::Face { body, point, .. } = selected {
            options.push((format!("Selected face on {body} (shared appearance)"), json!({"face_appearance":{"body":body,"point":point}})));
        }
    }
    let mut pick: Option<Value> = None;
    let filter = &mut rule_editor.column_filter;
    let was_open = rule_editor.column_menu_open;
    let face_appearance = crate::i18n::tr_label("Face Appearance");
    let shown = ui_kit::plus_header(ui, |ui| {
        // A design has many parameters, features and bodies: a filter narrows the list as the
        // Parameters dialog's does. It takes focus as the menu opens, so typing filters at once.
        let search = ui_kit::menu_filter(ui, ui.make_persistent_id("config_column_filter"), filter, "Filter");
        if !was_open {
            search.request_focus();
        }
        crate::scenario::publish_handle("dialog:menu:Add Column:filter", search.rect.center());
        let query = filter.trim().to_lowercase();
        let matching: Vec<&(String, Value)> = options.iter().filter(|(label, _)| query.is_empty() || label.to_lowercase().contains(&query)).collect();
        // Enter with one match left picks it, as in the command search.
        if matching.len() == 1 && search.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            pick = matching.first().map(|(_, p)| p.clone());
            ui.close();
        }
        let faces = query.is_empty() || face_appearance.to_lowercase().contains(&query);
        if matching.is_empty() && !faces {
            ui_kit::text(ui, crate::i18n::tr("config.column_filter_empty"), ui_kit::Text::Note("column_filter_empty"));
        }
        // The popup remembers its size: once the filter has emptied the list, a list that fills
        // again would be clipped to that small height, so a list that scrolls takes the full one.
        let list_height = crate::theme::Tokens::get().space_24 * 12.0;
        egui::ScrollArea::vertical().max_height(list_height).min_scrolled_height(list_height).show(ui, |ui| {
            for (label, p) in &matching {
                if ui_kit::button(ui, label.as_str(), ui_kit::Button::Normal).clicked() {
                    pick = Some(p.clone());
                    ui.close();
                }
            }
            if !faces {
                return;
            }
            ui_kit::action_menu(ui, "Face Appearance", |ui| {
                ui_kit::text(
                    ui,
                    crate::i18n::tr_label("Semantic faces share their appearance across instances."),
                    ui_kit::Text::Note("semantic_faces"),
                );
                let state = app.session.model.state();
                for body in &state.bodies {
                    if body.body.is_mesh() {
                        continue;
                    }
                    ui_kit::action_menu(ui, &format!("Faces on {}", body.name), |ui| {
                        if let Ok(faces) = body.body.faces((body.body.size() * 2e-3).max(1e-3)) {
                            let mut seen = std::collections::BTreeSet::new();
                            let mesh = body.mesh();
                            let mut points = std::collections::BTreeMap::new();
                            for (triangle, index) in mesh.triangles.iter().zip(&mesh.tri_face) {
                                if let std::collections::btree_map::Entry::Vacant(entry) = points.entry(*index as usize)
                                    && let Some([a, b, c]) = mesh.tri(triangle)
                                {
                                    entry.insert((a + b + c) * (1. / 3.));
                                }
                            }
                            for face in faces {
                                // A curved face centroid can lie on another surface.
                                // Pick an interior triangle point on this actual face.
                                let Some(point) = points.get(&face.index).copied() else { continue };
                                let Ok(target) = app.session.doc.configuration_face_target(body, face.index, point) else { continue };
                                let supported = if target.certificate.is_some() {
                                    // Capture read this exact face's current source certificate.
                                    // Enumerate its pieces once in the command, not per menu face.
                                    app.session.doc.certify_config_face(&target).is_ok()
                                } else {
                                    app.session.doc.configuration_face_indices(body, &target).is_ok()
                                };
                                if !supported || !seen.insert(target.name.clone()) {
                                    continue;
                                }
                                let Some((_, role)) = solidcraft_engine::doc::persistent_name::split_feature(&target.name) else { continue };
                                if ui_kit::button(ui, role, ui_kit::Button::Normal).clicked() {
                                    pick = Some(json!({"face_appearance":{"target":target}}));
                                    ui.close();
                                }
                            }
                        }
                    });
                }
            });
        });
    });
    rule_editor.column_menu_open = shown.is_some();
    if shown.is_none() {
        rule_editor.column_filter.clear();
    }
    if let Some(p) = pick {
        // The first column starts the table (with a Default row of the current values).
        let (id, p) = if columns.is_empty() {
            (
                "config.configure",
                if let Some(n) = p.get("param") {
                    json!({ "params": [n] })
                } else if let Some(n) = p.get("feature") {
                    json!({ "features": [n] })
                } else if let Some(n) = p.get("material") {
                    json!({ "materials": [n] })
                } else if let Some(n) = p.get("component_appearance") {
                    json!({ "component_appearances": [n] })
                } else if let Some(n) = p.get("face_appearance") {
                    json!({ "face_appearances": [n] })
                } else {
                    json!({ "appearances": [p.get("appearance")] })
                },
            )
        } else {
            ("config.column", p)
        };
        run(app, error, id, p);
    }
}

fn display_cell(kind: &str, value: &str) -> String {
    let appearance = matches!(kind, "appearance" | "component_appearance" | "face_appearance");
    // Empty cells: the default material, and an appearance's own "Default" keyword (it follows).
    if (kind == "material" && value.eq_ignore_ascii_case(DEFAULT_MATERIAL)) || (appearance && value.eq_ignore_ascii_case("Default")) {
        return String::new();
    }
    if appearance {
        solidcraft_engine::doc::config::appearance_cell(value)
            .ok()
            .flatten()
            .filter(|look| solidcraft_engine::doc::appearance::library(&look.name).as_ref() == Some(look))
            .map(|look| look.name)
            .unwrap_or_else(|| value.to_owned())
    } else {
        value.to_owned()
    }
}

fn value_control(ui: &mut egui::Ui, id: egui::Id, text: &mut String, kind: &str, material_names: &[String]) -> (egui::Response, Option<String>) {
    let appearance = matches!(kind, "appearance" | "component_appearance" | "face_appearance");
    let mut choice = None;
    let response = if kind == "material" || appearance {
        let follow = if kind == "appearance" {
            "Follow material"
        } else if appearance {
            "Follow inherited appearance"
        } else {
            "Default material"
        };
        ui_kit::library_cell(ui, id, text, follow, |ui| {
            if ui_kit::button(ui, crate::i18n::tr_label(follow), ui_kit::Button::Normal).clicked() {
                // An appearance's "Default" keyword follows; a material's is the default material.
                let follow_value = if appearance { "Default" } else { DEFAULT_MATERIAL };
                choice = Some(follow_value.to_owned());
                ui.close();
            }
            let choices: Vec<_> = if kind == "material" {
                material_names.iter().filter(|n| *n != DEFAULT_MATERIAL).map(String::as_str).collect()
            } else {
                solidcraft_engine::doc::appearance::LIBRARY.iter().map(|(name, _, _)| *name).collect()
            };
            for name in choices {
                if ui_kit::button(ui, name, ui_kit::Button::Normal).clicked() {
                    choice = Some(name.to_owned());
                    ui.close();
                }
            }
        })
    } else {
        ui_kit::cell_editor(ui, id, text, "", false)
    };
    (response, choice)
}
