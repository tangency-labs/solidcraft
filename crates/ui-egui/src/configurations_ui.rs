//! Manage configuration table. Commands and cell decoding remain in dialogs_motion.
use super::{cell_value, column_parts, configuration_rules_ui, table, unit_of};
use crate::ui_kit::BackingTooltipExt as _;
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
    let labels: Vec<_> = std::iter::once(solidcraft_ui_text::tr_label("Configuration").to_owned())
        .chain(tb.columns.iter().map(|c| {
            let (kind, name) = column_parts(c);
            v["targets"]
                .as_array()
                .and_then(|targets| targets.iter().find(|target| target["key"].as_str() == Some(c)))
                .and_then(|target| target["label"].as_str())
                .filter(|_| matches!(kind, "component_appearance" | "face_appearance"))
                .map(str::to_string)
                .unwrap_or_else(|| match kind {
                    "material" => format!("{}: {name}", solidcraft_ui_text::tr_label("Material")),
                    "appearance" => format!("{}: {name}", solidcraft_ui_text::tr_label("Appearance")),
                    "suppress" => format!("{}: {name}", solidcraft_ui_text::tr_label("Suppress")),
                    _ => name.to_owned(),
                })
        }))
        .chain(std::iter::once(solidcraft_ui_text::tr_label("Actions").to_owned()))
        .collect();
    let gap = ui.spacing().item_spacing.x;
    let name_width = tb.rows.iter().map(|(name, _)| ui_kit::ContentWidth::name(ui, name)).fold(0.0, f32::max);
    let status_width = (ui_kit::ContentWidth::label(ui, "Active")
        + gap
        + ui_kit::ContentWidth::button(ui, "Apply")
        + gap
        + ui_kit::ContentWidth::button(ui, tr!("config.update")))
    .max(ui_kit::ContentWidth::button(ui, "Activate"));
    // Widths the user dragged a column's header divider to, by column key, so a column keeps its
    // width when others are added or removed (T-693). The shared table stores them by index.
    let keys: Vec<&str> = std::iter::once(NAME_KEY).chain(tb.columns.iter().map(String::as_str)).collect();
    let dragged = column_widths(ui.ctx(), &keys);
    let t = solidcraft_theme::Tokens::get();
    let columns: Vec<_> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let min_width = if index == 0 {
                name_width + gap + status_width + t.space_24
            } else {
                // Room for the whole heading where it fits, so long feature and parameter names
                // read without hovering; a longer one elides and shows in full on hover.
                (ui_kit::row_height() * 2.0 + t.space_24).max((ui_kit::ContentWidth::label(ui, label) + t.space_24).min(t.form_card_width))
            };
            if index == labels.len() - 1 {
                ui_kit::Column::new(label, 0.0)
                    .icon_only()
                    .min_content_width(3.0 * solidcraft_theme::Tokens::get().control_height + solidcraft_theme::Tokens::get().space_12)
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
                let column =
                    ui_kit::Column::new(label, weight).flex().min_content_width(min_width).manual_width(dragged.get(index).copied().flatten());
                // The configuration names stay in view while the other columns scroll sideways.
                if index == 0 { column.frozen() } else { column }
            }
        })
        .collect();
    // Keyboard moves between the value cells: Enter down the column, Shift+Enter up, Tab and
    // Shift+Tab along the row and on to the next or previous row.
    let cells = egui::Id::unique("configuration_cells");
    let names: Vec<&str> = tb.rows.iter().map(|(name, _)| name.as_str()).collect();
    let target = |row: usize, column: usize, key: ui_kit::CellKey| -> Option<(String, String)> {
        let width = tb.columns.len();
        let at = |r: usize, c: usize| Some((names.get(r)?.to_string(), tb.columns.get(c)?.clone()));
        match key {
            ui_kit::CellKey::Down => at(row + 1, column),
            ui_kit::CellKey::Up => at(row.checked_sub(1)?, column),
            ui_kit::CellKey::Next if column + 1 < width => at(row, column + 1),
            ui_kit::CellKey::Next => at(row + 1, 0),
            ui_kit::CellKey::Previous if column > 0 => at(row, column - 1),
            ui_kit::CellKey::Previous => at(row.checked_sub(1)?, width.checked_sub(1)?),
        }
    };
    // A view choice, this viewer's own (T-690): configurations as rows, or as columns.
    let swapped = ui
        .horizontal(|ui| {
            column_menu(app, ui, error, &tb.columns, rule_editor);
            let mut swapped = ui.ctx().data_mut(|m| m.get_persisted::<bool>(swapped_id())).unwrap_or(false);
            let toggle = ui_kit::checkbox(ui, tr!("config.as_columns"), &mut swapped);
            crate::scenario::publish_handle("config:as_columns", toggle.rect.center());
            if toggle.changed() {
                ui.ctx().data_mut(|m| m.insert_persisted(swapped_id(), swapped));
                // An edit in progress is kept, as leaving its cell would keep it.
                for ((row, column), text) in std::mem::take(&mut rule_editor.drafts) {
                    *error = app.run("config.row", json!({"name":row,"values":{&column:cell_value(&column,&text)}})).err();
                }
            }
            swapped
        })
        .inner;
    ui.add_space(solidcraft_theme::Tokens::get().space_6);
    if swapped {
        swapped_table(app, ui, error, rule_editor, Shown { tb, v, labels: &labels, materials: &material_names });
    } else {
        ui_kit::table_custom(ui, "configuration_table", &columns, tr!("manage.empty"), |ui, table| {
            table.headers_resizable(ui, widths_by_index(), |ui, index| {
                if index == labels.len() - 1 {
                    true
                } else if index > 0 {
                    if let (Some(column), Some(label)) = (tb.columns.get(index - 1), labels.get(index))
                        && ui_kit::removable_header(ui, label)
                    {
                        remove_column(app, error, column);
                    }
                    true
                } else {
                    false
                }
            });
            let matches = v.get("matches").and_then(Value::as_bool) != Some(false);
            for (row_index, (name, values)) in tb.rows.iter().enumerate() {
                let active = tb.active.as_deref() == Some(name.as_str());
                // Rows off screen keep their space without building their cells; one with an edit
                // in progress stays live, so leaving its cell still commits the edit.
                let editing = rule_editor.drafts.keys().any(|(row, _)| row == name) || ui_kit::landing_in_row(ui.ctx(), cells, name);
                table.row_visible(ui, name, active, editing, |row| {
                    row.cell(|ui| {
                        // A measured name slot keeps status/actions inside the Configuration column.
                        ui.allocate_ui_with_layout(
                            egui::vec2(name_width, ui_kit::row_height()),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| match &mut rule_editor.renaming {
                                Some((old, new)) if old == name => {
                                    let id = ui.make_persistent_id(("configuration_rename", name));
                                    let focused = ui.memory(|m| m.has_focus(id) || m.had_focus_last_frame(id));
                                    let response = ui_kit::cell_editor(ui, id, new, "Name", false);
                                    crate::scenario::publish_handle(&format!("config:rename:{name}"), response.rect.center());
                                    let entered = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                                    if response.lost_focus() || entered {
                                        let (old, new) = (old.clone(), new.trim().to_owned());
                                        if !new.is_empty() && new != old {
                                            run(app, error, "config.row", json!({"name":old,"rename":new}));
                                        }
                                        rule_editor.renaming = None;
                                    } else if !response.has_focus() {
                                        response.request_focus();
                                    }
                                }
                                _ => {
                                    let response =
                                        ui_kit::name_cell_with_hint(ui, name, true, solidcraft_ui_text::tr_label("Double-click to rename"));
                                    crate::scenario::publish_handle(&format!("config:name:{name}"), response.rect.center());
                                    if response.double_clicked() {
                                        rule_editor.renaming = Some((name.clone(), name.clone()));
                                    }
                                }
                            },
                        );
                        status_controls(app, ui, name, active, matches, error, rule_editor);
                    });
                    for (column_index, (column, value)) in tb.columns.iter().zip(values).enumerate() {
                        row.cell(|ui| {
                            let goes = |key: ui_kit::CellKey| target(row_index, column_index, key);
                            value_cell(
                                app,
                                ui,
                                error,
                                rule_editor,
                                Cell { table: cells, row: name, column, value, materials: &material_names },
                                &goes,
                            );
                        });
                    }
                    row.cell(|ui| {
                        ui.horizontal(|ui| {
                            for (glyph, tip, side, target) in [
                                (ui_kit::MOVE_UP, "Move up", "up", row_index.checked_sub(1)),
                                (ui_kit::MOVE_DOWN, "Move down", "down", (row_index + 1 < tb.rows.len()).then_some(row_index + 1)),
                            ] {
                                let response = ui_kit::compact_button(ui, glyph, tip, target.is_some(), false);
                                crate::scenario::publish_handle(&format!("config:{side}:{name}"), response.rect.center());
                                if response.clicked()
                                    && let Some(target) = target
                                {
                                    run(app, error, "config.row", json!({"name":name,"index":target}));
                                }
                            }
                            if ui_kit::bin_button(ui, "Delete the row").clicked() {
                                run(app, error, "config.row", json!({"name":name,"delete":true}));
                            }
                        });
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
                                    if kind == "suppress" {
                                        let (response, toggled) = suppress_box(ui, column, &value);
                                        crate::scenario::publish_handle(&format!("config:new:cell:{column}"), response.rect.center());
                                        if let Some(on) = toggled {
                                            rule_editor.new_values.insert(column.clone(), on.to_string());
                                        }
                                        // Enter in the box adds the row, as it does from the draft's value cells.
                                        let focused = response.has_focus() || response.lost_focus();
                                        left |= response.lost_focus();
                                        commit |= focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                                        return;
                                    }
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
        remember_column_widths(ui.ctx(), &keys);
    }
    // What Update couldn't take into the row, right under the table it concerns.
    if let Some((_, missed)) = rule_editor.not_captured.as_ref().filter(|(at, _)| *at == app.session.revision) {
        let names: Vec<String> = missed
            .iter()
            .map(|key| match column_parts(key) {
                ("suppress", name) => format!("{} {name}", solidcraft_ui_text::tr_label("Suppress")),
                (_, name) => name.to_owned(),
            })
            .collect();
        ui_kit::text(ui, &tr!("config.not_captured", names = &names.join(", ")), ui_kit::Text::Report("not_captured", false));
    }
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

/// Where this viewer's choice to show configurations as columns is kept.
fn swapped_id() -> egui::Id {
    egui::Id::unique("sc_configurations_as_columns")
}

/// Remove `column` (a column key) from the table.
fn remove_column(app: &mut SolveApp, error: &mut Option<String>, column: &str) {
    let (kind, name) = column_parts(column);
    let key = if kind == "suppress" { "feature" } else { kind };
    *error = app.run("config.column", json!({key:if kind == "face_appearance" {column} else {name},"delete":true})).err();
}

/// The key the Configuration (name) column's width is kept under.
const NAME_KEY: &str = "configuration";

/// Where the dragged widths are kept by column key, across designs and sessions.
fn widths_by_key() -> egui::Id {
    egui::Id::unique("sc_configuration_column_widths")
}

/// Where the shared table reads and writes this frame's dragged widths, by column index.
fn widths_by_index() -> egui::Id {
    egui::Id::unique("sc_configuration_column_widths_shown")
}

/// The dragged width of each column of `keys`, in order, handed to the table by index.
fn column_widths(ctx: &egui::Context, keys: &[&str]) -> Vec<Option<f32>> {
    let kept: std::collections::BTreeMap<String, f32> = ctx.data_mut(|m| m.get_persisted(widths_by_key())).unwrap_or_default();
    let widths: Vec<Option<f32>> = keys.iter().map(|key| kept.get(*key).copied().filter(|w| w.is_finite() && *w > 0.0)).collect();
    ctx.data_mut(|m| m.insert_persisted(widths_by_index(), widths.clone()));
    widths
}

/// Keep a width the user dragged (or reset with a double-click) this frame under its column key.
fn remember_column_widths(ctx: &egui::Context, keys: &[&str]) {
    let shown: Vec<Option<f32>> = ctx.data_mut(|m| m.get_persisted(widths_by_index())).unwrap_or_default();
    ctx.data_mut(|m| {
        let mut kept: std::collections::BTreeMap<String, f32> = m.get_persisted(widths_by_key()).unwrap_or_default();
        let before = kept.clone();
        for (key, width) in keys.iter().zip(&shown) {
            match width {
                Some(width) if width.is_finite() && *width > 0.0 => {
                    kept.insert((*key).to_owned(), *width);
                }
                _ => {
                    kept.remove(*key);
                }
            }
        }
        // A design with hundreds of columns must not grow the kept list without bound.
        while kept.len() > 512 {
            let Some(first) = kept.keys().next().cloned() else { break };
            kept.remove(&first);
        }
        if kept != before {
            m.insert_persisted(widths_by_key(), kept);
        }
    });
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
    let face_appearance = solidcraft_ui_text::tr_label("Face Appearance");
    let shown = ui_kit::add_column_button(ui, |ui| {
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
            ui_kit::text(ui, tr!("config.column_filter_empty"), ui_kit::Text::Note("column_filter_empty"));
        }
        // The popup remembers its size: once the filter has emptied the list, a list that fills
        // again would be clipped to that small height, so a list that scrolls takes the full one.
        let list_height = solidcraft_theme::Tokens::get().space_24 * 12.0;
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
                    solidcraft_ui_text::tr_label("Semantic faces share their appearance across instances."),
                    ui_kit::Text::Note("semantic_faces"),
                );
                let state = app.session.model.state();
                for body in &state.bodies {
                    if body.body.is_mesh() {
                        continue;
                    }
                    ui_kit::action_menu(ui, &format!("Faces on {}", body.name), |ui| {
                        // Measured once per body, at the same tolerance: this menu is drawn again
                        // every frame while it is open, and measuring meshes the whole body.
                        if let Ok(faces) = body.joint_faces() {
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
                            for face in faces.iter() {
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

/// A suppression cell is a check box, ticked while the feature is suppressed, so the table reads
/// at a glance and one click or Space changes it. The `toggled` value is the new suppression;
/// Enter is left to move between cells, as it does from a value cell, rather than toggling.
fn suppress_box(ui: &mut egui::Ui, column: &str, value: &str) -> (egui::Response, Option<bool>) {
    let mut on = cell_value(column, value) == json!(true);
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let response = ui_kit::checkbox(ui, "", &mut on);
    // A clicked box keeps the keyboard, as a clicked value cell does, so Space, Tab and Enter
    // carry on from it.
    if response.clicked() {
        response.request_focus();
    }
    let toggled = response.changed() && !enter;
    (response, toggled.then_some(on))
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
            if ui_kit::button(ui, solidcraft_ui_text::tr_label(follow), ui_kit::Button::Normal).clicked() {
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

/// A configuration's status: the Active pill with Apply and Update while the design no longer
/// matches it, or Activate for another configuration.
fn status_controls(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    name: &str,
    active: bool,
    matches: bool,
    error: &mut Option<String>,
    rule_editor: &mut configuration_rules_ui::Editor,
) {
    if !active {
        let activate = ui_kit::button(ui, solidcraft_ui_text::tr_label("Activate"), ui_kit::Button::Small);
        crate::scenario::publish_handle(&format!("config:activate:{name}"), activate.rect.center());
        if activate.clicked() {
            *error = app.run_heavy("config.activate", json!({"row":name})).err();
        }
        return;
    }
    ui_kit::active_pill(ui);
    if matches {
        return;
    }
    let apply = ui_kit::button(ui, solidcraft_ui_text::tr_label("Apply"), ui_kit::Button::Small);
    crate::scenario::publish_handle(&format!("config:activate:{name}"), apply.rect.center());
    if apply.clicked() {
        *error = app.run_heavy("config.activate", json!({"row":name})).err();
    }
    // The other way round: the row takes the design's current values (T-686).
    let update = ui_kit::button(ui, tr!("config.update"), ui_kit::Button::Small).backing_on_hover_text(tr!("config.update_tip"));
    crate::scenario::publish_handle(&format!("config:update:{name}"), update.rect.center());
    if update.clicked() {
        match app.run("config.update_from_design", json!({"row":name})) {
            Ok(reply) => {
                *error = None;
                let missed: Vec<String> =
                    reply["not_captured"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
                rule_editor.not_captured = (!missed.is_empty()).then_some((app.session.revision, missed));
            }
            Err(e) => *error = Some(e),
        }
    }
}

/// One value cell of the table: a configuration (`row`) and one of the table's columns.
struct Cell<'a> {
    /// The keyboard-landing id shared by the table's cells.
    table: egui::Id,
    row: &'a str,
    column: &'a str,
    value: &'a str,
    materials: &'a [String],
}

/// Draw and edit one value cell. `goes` says where Enter, Tab and their Shift forms lead from it
/// (as a row name and a column key), which depends on which way round the table is shown.
fn value_cell(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    error: &mut Option<String>,
    rule_editor: &mut configuration_rules_ui::Editor,
    cell: Cell<'_>,
    goes: &dyn Fn(ui_kit::CellKey) -> Option<(String, String)>,
) {
    let Cell { table: cells, row: name, column, value, materials } = cell;
    let mut run = |app: &mut SolveApp, p: Value| {
        *error = app.run("config.row", p).err();
    };
    let tab_target = [ui_kit::CellKey::Next, ui_kit::CellKey::Previous].into_iter().any(|key| goes(key).is_some());
    let (kind, _) = column_parts(column);
    if kind == "suppress" {
        let (response, toggled) = suppress_box(ui, column, value);
        crate::scenario::publish_handle(&format!("config:cell:{name}:{column}"), response.rect.center());
        if let Some(on) = toggled {
            run(app, json!({"name":name,"values":{column:on}}));
        }
        // Enter, Tab and their Shift forms move on as from a value cell.
        let landed = ui_kit::landing(ui, cells, name, column, &response, "");
        let focused = response.has_focus() && !landed;
        if tab_target {
            ui_kit::keep_tab_in_cell(ui, &response);
        }
        if let Some((row, column)) = ui_kit::cell_key(ui, focused).and_then(goes) {
            ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            ui_kit::land_in_cell(ui.ctx(), cells, &row, &column);
        }
        return;
    }
    let shown = display_cell(kind, value);
    let key = (name.to_owned(), column.to_owned());
    let mut text = rule_editor.drafts.get(&key).cloned().unwrap_or_else(|| shown.clone());
    let id = ui.make_persistent_id(&key);
    let (response, choice) = value_control(ui, id, &mut text, kind, materials);
    // Escape blurs the editor before widgets render; discard rather than commit the draft.
    if ui_kit::text_edit_escape_owner(ui.ctx()) == Some(response.id) {
        rule_editor.drafts.remove(&key);
        return;
    }
    if let Some(choice) = choice {
        run(app, json!({"name":name,"values":{column:choice}}));
    }
    crate::scenario::publish_handle(&format!("config:cell:{name}:{column}"), response.rect.center());
    // A cell the keyboard has just landed in leaves this pass's key to the cell it came from.
    let landed = ui_kit::landing(ui, cells, name, column, &response, &text);
    let focused = response.has_focus() && !landed;
    let completed = crate::params_dialog::complete(ui, &response, &mut text);
    if response.changed() || completed {
        rule_editor.drafts.insert(key.clone(), text.clone());
    }
    let moved = if completed { None } else { ui_kit::cell_key(ui, focused || (response.lost_focus() && !landed)) };
    let to = moved.and_then(goes);
    // Tab stays inside the table while there is a cell to go to; at either end it leaves the
    // table in egui's order.
    if tab_target {
        ui_kit::keep_tab_in_cell(ui, &response);
    }
    let enter = !completed && focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    let tabbed = focused && matches!(moved, Some(ui_kit::CellKey::Next | ui_kit::CellKey::Previous)) && to.is_some();
    if response.lost_focus() || enter || tabbed {
        rule_editor.drafts.remove(&key);
        if text != shown {
            run(app, json!({"name":name,"values":{column:cell_value(column,&text)}}));
        }
        if let Some((row, column)) = to {
            ui_kit::land_in_cell(ui.ctx(), cells, &row, &column);
        }
    }
}

/// What the swapped table shows: the table, `config.table`'s answer, each column's heading
/// (`labels[0]` is the Configuration heading, the last the actions') and the materials offered.
struct Shown<'a> {
    tb: &'a super::Table,
    v: &'a Value,
    labels: &'a [String],
    materials: &'a [String],
}

/// The table the other way round (T-690): a row per parameter, feature or look, a column per
/// configuration, for designs with many columns and few configurations. The same cells and
/// commands as the usual table; Enter moves down to the next item and Tab across to the next
/// configuration. Only the view changes, nothing in the design.
fn swapped_table(
    app: &mut SolveApp,
    ui: &mut egui::Ui,
    error: &mut Option<String>,
    rule_editor: &mut configuration_rules_ui::Editor,
    shown: Shown<'_>,
) {
    let Shown { tb, v, labels, materials } = shown;
    let t = solidcraft_theme::Tokens::get();
    let gap = ui.spacing().item_spacing.x;
    let matches = v.get("matches").and_then(Value::as_bool) != Some(false);
    let items = labels.get(1..labels.len().saturating_sub(1)).unwrap_or_default();
    let item_width = items.iter().map(|label| ui_kit::ContentWidth::label(ui, label)).fold(t.form_label_width, f32::max).min(t.form_card_width);
    let status_width = (ui_kit::ContentWidth::label(ui, "Active")
        + gap
        + ui_kit::ContentWidth::button(ui, "Apply")
        + gap
        + ui_kit::ContentWidth::button(ui, tr!("config.update")))
    .max(ui_kit::ContentWidth::button(ui, "Activate"))
        + gap
        + t.control_height
        // The Active pill's own padding, beyond its label.
        + t.space_24 * 2.0;
    let keys: Vec<String> =
        std::iter::once(SWAPPED_ITEM_KEY.to_owned()).chain(tb.rows.iter().map(|(name, _)| format!("{SWAPPED_ROW_KEY}{name}"))).collect();
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
    let dragged = column_widths(ui.ctx(), &key_refs);
    let mut columns = vec![
        ui_kit::Column::new("", 1.2).flex().min_content_width(item_width + t.space_24).manual_width(dragged.first().copied().flatten()).frozen(),
    ];
    for (index, (name, _)) in tb.rows.iter().enumerate() {
        let width = (ui_kit::ContentWidth::name(ui, name) + t.space_24).max(status_width).max(ui_kit::row_height() * 2.0 + t.space_24);
        columns.push(ui_kit::Column::new(name, 1.0).flex().min_content_width(width).manual_width(dragged.get(index + 1).copied().flatten()));
    }
    columns.push(ui_kit::Column::new("", 0.0).icon_only().min_content_width(t.control_height + t.space_12));
    let cells = egui::Id::unique("configuration_cells");
    let names: Vec<&str> = tb.rows.iter().map(|(name, _)| name.as_str()).collect();
    // `config` indexes the configurations (the table's rows), `item` its columns.
    let target = |config: usize, item: usize, key: ui_kit::CellKey| -> Option<(String, String)> {
        let at = |c: usize, i: usize| Some((names.get(c)?.to_string(), tb.columns.get(i)?.clone()));
        match key {
            ui_kit::CellKey::Down => at(config, item + 1),
            ui_kit::CellKey::Up => at(config, item.checked_sub(1)?),
            ui_kit::CellKey::Next if config + 1 < names.len() => at(config + 1, item),
            ui_kit::CellKey::Next => at(0, item + 1),
            ui_kit::CellKey::Previous if config > 0 => at(config - 1, item),
            ui_kit::CellKey::Previous => at(names.len().checked_sub(1)?, item.checked_sub(1)?),
        }
    };
    // A keyboard move may land in a row off screen: build every row until it has.
    let landing = names.iter().any(|name| ui_kit::landing_in_row(ui.ctx(), cells, name));
    ui_kit::table_custom(ui, "configuration_table_swapped", &columns, tr!("manage.empty"), |ui, table| {
        table.headers_resizable(ui, widths_by_index(), |ui, index| {
            if let Some(name) = names.get(index.wrapping_sub(1)) {
                ui_kit::text(ui, name, ui_kit::Text::NameCell(true));
            }
            true
        });
        table.row(ui, "status", false, |row| {
            row.cell(|_| {});
            for name in &names {
                row.cell(|ui| {
                    let active = tb.active.as_deref() == Some(*name);
                    status_controls(app, ui, name, active, matches, error, rule_editor);
                    if ui_kit::bin_button(ui, tr!("config.delete_configuration")).clicked() {
                        // Its cells are drawn after this one: an edit in one of them goes with it,
                        // rather than making the configuration again as the cell is left.
                        rule_editor.drafts.retain(|(row, _), _| row.as_str() != *name);
                        *error = app.run("config.row", json!({"name":name,"delete":true})).err();
                    }
                });
            }
            row.cell(|_| {});
        });
        for (item, column) in tb.columns.iter().enumerate() {
            let editing = landing || rule_editor.drafts.keys().any(|(_, at)| at == column);
            table.row_visible(ui, column, false, editing, |row| {
                row.cell(|ui| {
                    ui_kit::text(ui, items.get(item).map_or(column.as_str(), String::as_str), ui_kit::Text::Cell);
                });
                for (config, (name, values)) in tb.rows.iter().enumerate() {
                    row.cell(|ui| {
                        let value = values.get(item).map_or("", String::as_str);
                        let goes = |key: ui_kit::CellKey| target(config, item, key);
                        value_cell(app, ui, error, rule_editor, Cell { table: cells, row: name, column, value, materials }, &goes);
                    });
                }
                row.cell(|ui| {
                    if ui_kit::bin_button(ui, "Remove the column").clicked() {
                        remove_column(app, error, column);
                    }
                });
            });
        }
        if table.add_prompt(ui, "Add configuration", "A new configuration, a copy of the active one").clicked() {
            let name = (tb.rows.len() + 1..).map(|k| format!("Configuration{k}")).find(|n| !tb.rows.iter().any(|r| &r.0 == n)).unwrap_or_default();
            let mut params = json!({"name":name});
            if let Some(active) = &tb.active {
                params["from"] = json!(active);
            }
            *error = app.run("config.row", params).err();
        }
    });
    remember_column_widths(ui.ctx(), &key_refs);
}

/// The keys the swapped table's dragged widths are kept under: its item column, and each
/// configuration's column by the configuration's name.
const SWAPPED_ITEM_KEY: &str = "swapped:item";
const SWAPPED_ROW_KEY: &str = "swapped:row:";
