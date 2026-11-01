//! Personal Sheet Metal Rules: rules kept in the user's profile and offered by the Sheet Metal
//! Rules manager of every design (#9346). Picking one only fills the open manager; the manager's
//! OK adds it to the design, or updates the design's rule of that name, as for any rule. The
//! design keeps its own copy, so it builds the same where the profile has no such rule.

use serde::{Deserialize, Serialize, de};
use solidcraft_engine::doc::sheet::{AngleDisplay, ReliefShape, SheetRule, UnfoldMethod};
use solidcraft_engine::doc::{DocError, Document, expr};
use std::collections::BTreeMap;

use crate::dialogs::{Dialog, Kind};
use crate::dialogs_sheet::{ANGLE_DISPLAYS, RELIEF_SHAPES, RULE_FIELDS, Sm, UNFOLD_METHODS, design_rules};
use crate::i18n::tr;
use crate::ui_kit::{self, Text};

const MAX_RULES: usize = 64;
/// The longest rule name `sheet.manage_rules` takes.
const MAX_NAME: usize = 128;
const MAX_EXPRESSION: usize = 512;

const INVALID_NAME: &str = "sheet.personal_rules.invalid_name";
const INVALID_VALUES: &str = "sheet.personal_rules.invalid_values";

/// A personal rule: a rule's name and values. Every length carries its unit, so the rule gives
/// the same sheet in a millimetre or an inch design; `Thickness` and other expressions are kept
/// as written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Rule {
    name: String,
    thickness: String,
    k_factor: String,
    bend_radius: String,
    relief_width: String,
    relief_depth: String,
    corner_relief: String,
    corner_relief_shape: String,
    hem_gap: String,
    gap: String,
    angle_display: String,
    /// What sets the K-factor (`SheetRule::unfold_method`). Saved only when it isn't the K-factor,
    /// so a library without such rules reads the same in versions before the setting.
    #[serde(default = "k_factor_method", skip_serializing_if = "is_k_factor_method")]
    unfold_method: String,
    /// The 90° bend deduction or allowance, with its unit; empty, and not saved, for the K-factor.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    unfold_value: String,
    /// Values a newer version saved. They survive renaming and are never dropped silently: a rule
    /// that has them can't be recalled or overwritten here.
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

fn k_factor_method() -> String {
    UnfoldMethod::KFactor.name().into()
}

fn is_k_factor_method(method: &str) -> bool {
    UnfoldMethod::parse(method) == Some(UnfoldMethod::KFactor)
}

fn valid_text(text: &str, max: usize) -> bool {
    text.len() <= max && !text.chars().any(char::is_control)
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name == name.trim() && valid_text(name, MAX_NAME)
}

/// The manager's text for a value, by its parameter name (`dialogs_sheet::RULE_FIELDS`).
fn field<'a>(values: &'a [String], key: &str) -> &'a str {
    RULE_FIELDS.iter().position(|(k, _)| *k == key).and_then(|i| values.get(i)).map_or("", |v| v.trim())
}

/// A length as the rule keeps it: a bare number takes the design's unit, and an expression
/// without a unit is read in it, as the manager's OK would; anything else is kept as written.
fn portable_length(text: &str, unit: &str, lookup: &dyn Fn(&str) -> Result<expr::Value, DocError>) -> Result<String, &'static str> {
    let text = text.trim();
    // A definition (`T = 2`) would name a parameter of this design only.
    if text.is_empty() || !valid_text(text, MAX_EXPRESSION) || expr::definition(text).is_some() {
        return Err(INVALID_VALUES);
    }
    if expr::is_bare_number(text) {
        return Ok(format!("{text} {unit}"));
    }
    let value = expr::eval_with(text, lookup).map_err(|_| INVALID_VALUES)?;
    Ok(if value.unitless() { format!("({text}) * 1 {unit}") } else { text.into() })
}

impl Rule {
    /// A value by its parameter name, as `dialogs_sheet::RULE_FIELDS` names them.
    fn value(&self, key: &str) -> Option<&String> {
        Some(match key {
            "thickness" => &self.thickness,
            "k_factor" => &self.k_factor,
            "bend_radius" => &self.bend_radius,
            "relief_width" => &self.relief_width,
            "relief_depth" => &self.relief_depth,
            "corner_relief" => &self.corner_relief,
            "hem_gap" => &self.hem_gap,
            "gap" => &self.gap,
            "unfold_value" => &self.unfold_value,
            _ => return None,
        })
    }

    fn expressions(&self) -> [&String; 8] {
        [&self.thickness, &self.k_factor, &self.bend_radius, &self.relief_width, &self.relief_depth, &self.corner_relief, &self.hem_gap, &self.gap]
    }

    fn sheet_rule(&self) -> SheetRule {
        SheetRule {
            name: self.name.clone(),
            thickness: self.thickness.clone(),
            k_factor: self.k_factor.clone(),
            bend_radius: self.bend_radius.clone(),
            relief_width: self.relief_width.clone(),
            relief_depth: self.relief_depth.clone(),
            corner_relief: self.corner_relief.clone(),
            corner_relief_shape: self.corner_relief_shape.clone(),
            hem_gap: self.hem_gap.clone(),
            gap: self.gap.clone(),
            angle_display: self.angle_display.clone(),
            unfold_method: self.unfold_method.clone(),
            unfold_value: self.unfold_value.clone(),
        }
    }

    /// The shape of a stored row. Values are evaluated only in a design (capture and the manager's
    /// OK), never when the preferences load.
    fn validate(&self) -> Result<(), &'static str> {
        if !valid_name(&self.name) {
            return Err(INVALID_NAME);
        }
        if self.expressions().into_iter().any(|text| text.trim().is_empty() || !valid_text(text, MAX_EXPRESSION))
            || ReliefShape::parse(&self.corner_relief_shape).is_none()
            || AngleDisplay::parse(&self.angle_display).is_none()
            || !self.unfold_valid()
            || self.unknown.len() > 8
            || self.unknown.iter().any(|(key, value)| {
                key.is_empty()
                    || !valid_text(key, 64)
                    || match value {
                        serde_json::Value::String(text) => !valid_text(text, MAX_EXPRESSION),
                        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Null => false,
                        _ => true,
                    }
            })
        {
            return Err(INVALID_VALUES);
        }
        Ok(())
    }

    /// A known unfold method, with a 90° value exactly when the method reads one.
    fn unfold_valid(&self) -> bool {
        match UnfoldMethod::parse(&self.unfold_method) {
            Some(UnfoldMethod::KFactor) => self.unfold_value.is_empty(),
            Some(_) => !self.unfold_value.trim().is_empty() && valid_text(&self.unfold_value, MAX_EXPRESSION),
            None => false,
        }
    }

    /// The rule the manager shows, under `name`. The design must accept it as the manager's OK
    /// would: a positive thickness and a K factor from 0 to 1.
    fn capture(sm: &Sm, name: &str, doc: &Document) -> Result<Self, &'static str> {
        let Sm::Rules { values, relief, angles, method, .. } = sm else {
            return Err(INVALID_VALUES);
        };
        let (vals, _) = doc.param_values();
        let unit = expr::length_unit_or_mm(&doc.units);
        let lookup = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        let thickness = portable_length(field(values, "thickness"), unit, &lookup)?;
        let t = expr::eval_with(&thickness, &lookup).and_then(|v| v.to_kind(expr::Kind::Length)).map_err(|_| INVALID_VALUES)?;
        // The other values may use the rule's own thickness, as `rule_values` reads them.
        let with_t = |n: &str| if n == "Thickness" && !vals.contains_key("Thickness") { Ok(expr::Value::length(t)) } else { lookup(n) };
        let length = |key: &str| portable_length(field(values, key), unit, &with_t);
        let k_factor = field(values, "k_factor");
        if k_factor.is_empty() || !valid_text(k_factor, MAX_EXPRESSION) || expr::definition(k_factor).is_some() {
            return Err(INVALID_VALUES);
        }
        // The K-factor method reads no 90° value, so one left in its hidden field isn't kept.
        let unfold_method = UNFOLD_METHODS.get(*method).copied().unwrap_or("k_factor");
        let unfold_value = if is_k_factor_method(unfold_method) { String::new() } else { length("unfold_value")? };
        let rule = Self {
            name: name.trim().into(),
            thickness,
            k_factor: k_factor.into(),
            bend_radius: length("bend_radius")?,
            relief_width: length("relief_width")?,
            relief_depth: length("relief_depth")?,
            corner_relief: length("corner_relief")?,
            corner_relief_shape: RELIEF_SHAPES.get(*relief).copied().unwrap_or("notch").into(),
            hem_gap: length("hem_gap")?,
            gap: length("gap")?,
            angle_display: ANGLE_DISPLAYS.get(*angles).copied().unwrap_or("bend").into(),
            unfold_method: unfold_method.into(),
            unfold_value,
            unknown: BTreeMap::new(),
        };
        rule.validate()?;
        doc.rule_values(&vals, &rule.sheet_rule()).map_err(|_| INVALID_VALUES)?;
        Ok(rule)
    }

    fn same_values(&self, other: &Self) -> bool {
        let values = |rule: &Self| {
            let mut rule = rule.clone();
            rule.name.clear();
            rule.unknown.clear();
            rule
        };
        values(self) == values(other)
    }

    /// Fill the manager with this rule: the design's rule of the same name when it has one, else
    /// a new rule by this name. Nothing changes in the design until the manager's OK.
    fn recall(&self, sm: &mut Sm, doc: &Document) -> Result<(), &'static str> {
        self.validate()?;
        if !self.unknown.is_empty() {
            return Err("sheet.personal_rules.unsupported");
        }
        let Sm::Rules { pick, name, values, relief, angles, method, active, loaded } = sm else {
            return Err(INVALID_VALUES);
        };
        let rules = design_rules(doc);
        *pick = rules.iter().position(|r| r.name == self.name).unwrap_or(rules.len());
        name.clone_from(&self.name);
        *values = RULE_FIELDS.iter().map(|(key, _)| self.value(key).cloned().unwrap_or_default()).collect();
        *relief = RELIEF_SHAPES.iter().position(|s| s.eq_ignore_ascii_case(self.corner_relief_shape.trim())).unwrap_or(0);
        *angles = ANGLE_DISPLAYS.iter().position(|d| d.eq_ignore_ascii_case(self.angle_display.trim())).unwrap_or(0);
        *method = UNFOLD_METHODS.iter().position(|m| m.eq_ignore_ascii_case(self.unfold_method.trim())).unwrap_or(0);
        *active = doc.sheet_rule(None).name == self.name || *pick >= rules.len();
        // The manager loads a rule's values only when its Rule box changes: these are now its own.
        *loaded = Some(*pick);
        Ok(())
    }
}

/// Stored as one preference array. A malformed row cannot discard unrelated preferences or valid
/// rows. The rows and every user-controlled string are bounded.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Library(Vec<Rule>);

impl<'de> Deserialize<'de> for Library {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LibraryVisitor;
        impl<'de> de::Visitor<'de> for LibraryVisitor {
            type Value = Library;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an array of personal sheet metal rules")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Library, A::Error> {
                let mut library = Library::default();
                for _ in 0..MAX_RULES {
                    let Some(row) = seq.next_element::<serde_json::Value>()? else { return Ok(library) };
                    if let Ok(rule) = serde_json::from_value::<Rule>(row)
                        && rule.validate().is_ok()
                        && library.find(&rule.name).is_none()
                    {
                        library.0.push(rule);
                    }
                }
                while seq.next_element::<de::IgnoredAny>()?.is_some() {}
                Ok(library)
            }
        }
        deserializer.deserialize_seq(LibraryVisitor)
    }
}

impl Library {
    /// Names are unique ignoring case.
    fn find(&self, name: &str) -> Option<usize> {
        let name = name.to_lowercase();
        self.0.iter().position(|rule| rule.name.to_lowercase() == name)
    }

    fn save(&mut self, sm: &Sm, name: &str, doc: &Document) -> Result<(), &'static str> {
        if self.find(name.trim()).is_some() {
            return Err("sheet.personal_rules.name_exists");
        }
        if self.0.len() >= MAX_RULES {
            return Err("sheet.personal_rules.full");
        }
        self.0.push(Rule::capture(sm, name, doc)?);
        Ok(())
    }

    fn modified(&self, selected: &str, sm: &Sm, doc: &Document) -> bool {
        self.find(selected).and_then(|index| self.0.get(index)).is_some_and(|saved| match Rule::capture(sm, &saved.name, doc) {
            Ok(current) => !current.same_values(saved),
            Err(_) => true,
        })
    }

    fn overwrite(&mut self, selected: &str, sm: &Sm, doc: &Document) -> Result<(), &'static str> {
        let index = self.find(selected).ok_or("sheet.personal_rules.missing")?;
        let saved = self.0.get(index).ok_or("sheet.personal_rules.missing")?;
        if !saved.unknown.is_empty() {
            return Err("sheet.personal_rules.unsupported");
        }
        let candidate = Rule::capture(sm, &saved.name, doc)?;
        // Capture and its checks finish before the saved rule is replaced.
        *self.0.get_mut(index).ok_or("sheet.personal_rules.missing")? = candidate;
        Ok(())
    }

    fn rename(&mut self, old: &str, new: &str) -> Result<(), &'static str> {
        let new = new.trim();
        if !valid_name(new) {
            return Err(INVALID_NAME);
        }
        let index = self.find(old).ok_or("sheet.personal_rules.missing")?;
        if self.find(new).is_some_and(|other| other != index) {
            return Err("sheet.personal_rules.name_exists");
        }
        let rule = self.0.get_mut(index).ok_or("sheet.personal_rules.missing")?;
        rule.name = new.into();
        Ok(())
    }
}

/// The name of the rule the manager shows: the design's rule picked, or the new rule's name.
fn shown_name(sm: &Sm, doc: &Document) -> String {
    let Sm::Rules { pick, name, .. } = sm else { return String::new() };
    design_rules(doc).get(*pick).map_or_else(|| name.trim().to_string(), |rule| rule.name.clone())
}

#[derive(Clone)]
struct NamePrompt {
    name: String,
    renaming: bool,
}

#[derive(Clone, Default)]
struct PanelState {
    selected: String,
    prompt: Option<NamePrompt>,
    confirming_delete: bool,
    error: Option<&'static str>,
}

fn panel_id() -> egui::Id {
    egui::Id::new("personal_sheet_rules_panel")
}

fn rule_manager(d: &mut Dialog) -> Option<&mut Sm> {
    match &mut d.kind {
        Kind::Sheet(sm @ Sm::Rules { .. }) => Some(sm),
        _ => None,
    }
}

/// One section above the Sheet Metal Rules manager's fields. Only the library is kept in the
/// preferences; the selection, errors and the naming prompt belong to this dialog.
pub(crate) fn rows(ui: &mut egui::Ui, d: &mut Dialog, library: &mut Library, doc: &Document, reset: bool) {
    let Some(sm) = rule_manager(d) else { return };
    let id = panel_id();
    let mut state = if reset { PanelState::default() } else { ui.ctx().data_mut(|data| data.get_temp::<PanelState>(id)).unwrap_or_default() };
    ui_kit::section(ui, "personal_sheet_rules", "sheet.personal_rules.title", false, |ui| {
        ui.label(tr("sheet.personal_rules.scope"));
        crate::dialogs::form_row(ui, "sheet.personal_rules.rule", |ui| {
            let before = library.find(&state.selected).map_or(0, |index| index + 1);
            let mut selected = before;
            let dirty = library.modified(&state.selected, sm, doc);
            let display = library.find(&state.selected).and_then(|index| library.0.get(index)).map_or_else(
                || tr("sheet.personal_rules.select").to_string(),
                |rule| {
                    if dirty { format!("* {}", rule.name) } else { rule.name.clone() }
                },
            );
            let height = ui_kit::row_height();
            let width = (ui.available_width() - height - ui.spacing().item_spacing.x).max(1.0);
            ui.allocate_ui_with_layout(egui::vec2(width, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let response = ui_kit::dropdown_menu(ui, "personal_sheet_rule_choice", &display, |ui| {
                    let placeholder = ui_kit::list_picker(ui, "personal_sheet_rules_select", tr("sheet.personal_rules.select"), selected == 0);
                    if placeholder.clicked() {
                        selected = 0;
                    }
                    crate::scenario::publish_handle(
                        &format!("dialog:sheet.personal_rules.rule:{}", tr("sheet.personal_rules.select")),
                        placeholder.rect.center(),
                    );
                    for (index, rule) in library.0.iter().enumerate() {
                        let option = ui_kit::list_picker(ui, &format!("personal_sheet_rule_{index}"), &rule.name, selected == index + 1);
                        if option.clicked() {
                            selected = index + 1;
                        }
                        crate::scenario::publish_handle(&format!("dialog:sheet.personal_rules.rule:{}", rule.name), option.rect.center());
                    }
                });
                response.widget_info(|| {
                    let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), tr("sheet.personal_rules.rule"));
                    info.current_text_value = Some(display.clone());
                    info
                });
            });
            if selected != before {
                let rule = selected.checked_sub(1).and_then(|index| library.0.get(index));
                state.selected = rule.map_or_else(String::new, |rule| rule.name.clone());
                state.error = rule.and_then(|rule| rule.recall(sm, doc).err());
                state.prompt = None;
                state.confirming_delete = false;
            }
            let modified = library.modified(&state.selected, sm, doc);
            ui.add_enabled_ui(modified, |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(height, height), egui::Sense::hover());
                let save = ui_kit::icon_button_at(ui, rect, "save", tr("sheet.personal_rules.overwrite"));
                crate::scenario::publish_target("dialog:sheet.personal_rules:overwrite", save.rect);
                if save.clicked() {
                    state.error = library.overwrite(&state.selected, sm, doc).err();
                    state.confirming_delete = false;
                }
            });
        });
        let chosen = library.find(&state.selected);
        // Each button is also a handle (`dialog:sheet.personal_rules:<action>`) for scenarios and the
        // control channel.
        let button = |ui: &mut egui::Ui, action: &str, label: &str, enabled: bool| {
            let response = ui_kit::button(ui, label, ui_kit::Button::Enabled(enabled));
            crate::scenario::publish_handle(&format!("dialog:sheet.personal_rules:{action}"), response.rect.center());
            response.clicked()
        };
        ui.horizontal_wrapped(|ui| {
            if button(ui, "save", tr("sheet.personal_rules.save"), true) {
                state.prompt = Some(NamePrompt { name: shown_name(sm, doc), renaming: false });
                state.error = None;
                state.confirming_delete = false;
            }
            if button(ui, "rename", tr("sheet.personal_rules.rename"), chosen.is_some()) {
                state.prompt = Some(NamePrompt { name: state.selected.clone(), renaming: true });
                state.error = None;
                state.confirming_delete = false;
            }
            if button(ui, "delete", tr("sheet.personal_rules.delete"), chosen.is_some()) {
                state.confirming_delete = true;
                state.error = None;
            }
        });
        if state.confirming_delete {
            ui.label(tr("sheet.personal_rules.delete_prompt"));
            ui.horizontal_wrapped(|ui| {
                if button(ui, "confirm_delete", tr("sheet.personal_rules.confirm_delete"), true) {
                    if let Some(index) = library.find(&state.selected) {
                        library.0.remove(index);
                    }
                    state = PanelState::default();
                }
                if ui_kit::button(ui, tr("ui.cancel"), ui_kit::Button::Normal).clicked() {
                    state.confirming_delete = false;
                }
            });
        }
        if state.prompt.is_none()
            && let Some(error) = state.error
        {
            ui_kit::text(ui, tr(error), Text::Report("personal_sheet_rules_error", true));
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}

/// Global shortcuts run before dialogs. A pending name prompt owns their keyboard even when
/// Escape has already left its text field. Stale state cannot block other tools.
pub(crate) fn name_prompt_open(ctx: &egui::Context, dialog: Option<&Dialog>) -> bool {
    dialog.is_some_and(|d| matches!(&d.kind, Kind::Sheet(Sm::Rules { .. })))
        && ctx.data_mut(|data| data.get_temp::<PanelState>(panel_id())).is_some_and(|state| state.prompt.is_some())
}

/// Drawn after the manager's card, so the prompt owns input and its footer handles. `true` also
/// covers the prompt's frame: its Enter and Cancel never reach the manager.
pub(crate) fn name_prompt(ctx: &egui::Context, d: &mut Dialog, library: &mut Library, doc: &Document) -> bool {
    let Some(sm) = rule_manager(d) else { return false };
    let id = panel_id();
    let mut state = ctx.data_mut(|data| data.get_temp::<PanelState>(id)).unwrap_or_default();
    let Some(mut prompt) = state.prompt.take() else { return false };
    let title = tr(if prompt.renaming { "sheet.personal_rules.rename" } else { "sheet.personal_rules.save" });
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("personal_sheet_rule_name"), "save", title)
        .actions(if prompt.renaming { tr("sheet.personal_rules.rename") } else { crate::i18n::tr_label("Save") }, "Cancel", None)
        .show(ctx, true, |ui| {
            ui_kit::row(ui, "sheet.personal_rules.name", |ui| ui_kit::focused_text_field(ui, &mut prompt.name, tr("sheet.personal_rules.name_hint")));
            // Keep an over-long name for a clear error, but bound what the prompt holds.
            if prompt.name.len() > MAX_EXPRESSION {
                let end = prompt.name.char_indices().map(|(index, _)| index).take_while(|index| *index <= MAX_EXPRESSION).last().unwrap_or(0);
                prompt.name.truncate(end);
            }
            if let Some(error) = state.error {
                ui_kit::text(ui, tr(error), Text::Report("personal_sheet_rules_name_error", true));
            }
        });
    ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
    });
    if closed || action == ui_kit::Action::Cancel {
        state.error = None;
    } else if action == ui_kit::Action::Ok {
        state.error = if prompt.renaming { library.rename(&state.selected, &prompt.name).err() } else { library.save(sm, &prompt.name, doc).err() };
        if state.error.is_none() {
            state.selected = prompt.name.trim().into();
        } else {
            state.prompt = Some(prompt);
        }
    } else {
        state.prompt = Some(prompt);
    }
    ctx.data_mut(|data| data.insert_temp(id, state));
    true
}

#[cfg(test)]
#[path = "personal_sheet_rules_tests.rs"]
mod tests;
