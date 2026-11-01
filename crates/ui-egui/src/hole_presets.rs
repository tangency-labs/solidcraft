//! Personal, cross-design Hole parameter presets. Geometry references never enter this
//! library; recalling a preset only edits the open dialog, never the document.

use serde::{Deserialize, Serialize, de};
use solidcraft_engine::doc::{DocError, Document, expr};
use std::collections::BTreeMap;

use crate::dialogs::{Dialog, Kind};
use crate::i18n::tr;
use crate::ui_kit::{self, Button, Text};

const MAX_PRESETS: usize = 64;
const MAX_NAME: usize = 96;
const MAX_EXPRESSION: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Style {
    Simple,
    Drilled,
    Counterbore,
    Countersink,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Extent {
    Distance,
    All,
    ToObject,
}

/// Additive thread options retain their defaults in older presets. Known options map to
/// the Hole form without design IDs; unknown options survive rename and refuse recall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct ThreadOptions {
    /// `right` or `left`, under the name v0.24.0 saved it with.
    handedness: String,
    full_length: bool,
    depth: Option<String>,
    /// The thread cut into the wall, not only listed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    modeled: bool,
    /// A modelled thread's inner end: none is square; `standard` or `short` is ISO 3508's
    /// runout, any other text a fade length.
    #[serde(skip_serializing_if = "Option::is_none")]
    ease_out: Option<String>,
    /// A drill other than the tap maker's for the size; none: the tap maker's.
    #[serde(skip_serializing_if = "Option::is_none")]
    drill: Option<String>,
    #[serde(flatten)]
    unknown: BTreeMap<String, serde_json::Value>,
}

impl Default for ThreadOptions {
    fn default() -> Self {
        Self { handedness: "right".into(), full_length: true, depth: None, modeled: false, ease_out: None, drill: None, unknown: BTreeMap::new() }
    }
}

/// ISO 3508's runouts by name, not a typed length.
fn named_runout(text: &str) -> bool {
    matches!(text, "standard" | "short")
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Preset {
    name: String,
    style: Style,
    #[serde(default)]
    angled_bottom: bool,
    extent: Extent,
    diameter: String,
    depth: String,
    cb_diameter: String,
    cb_depth: String,
    cs_diameter: String,
    cs_angle: String,
    tip_angle: String,
    tapped: bool,
    thread: String,
    #[serde(default)]
    thread_options: ThreadOptions,
}

fn valid_text(text: &str, max: usize) -> bool {
    text.len() <= max && !text.chars().any(char::is_control)
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name == name.trim() && valid_text(name, MAX_NAME)
}

impl Preset {
    fn validate(&self) -> Result<(), &'static str> {
        if !valid_name(&self.name) {
            return Err("hole_presets.invalid_name");
        }
        let expressions =
            [&self.diameter, &self.depth, &self.cb_diameter, &self.cb_depth, &self.cs_diameter, &self.cs_angle, &self.tip_angle, &self.thread];
        if expressions.into_iter().any(|text| !valid_text(text, MAX_EXPRESSION)) {
            return Err("hole_presets.invalid_parameters");
        }
        let mut required = if self.tapped { Vec::new() } else { vec![&self.diameter] };
        if self.extent == Extent::Distance {
            required.push(&self.depth);
        }
        match self.style {
            Style::Drilled if self.extent != Extent::All => required.push(&self.tip_angle),
            Style::Counterbore => required.extend([&self.cb_diameter, &self.cb_depth]),
            Style::Countersink => required.extend([&self.cs_diameter, &self.cs_angle]),
            Style::Simple | Style::Drilled => {}
        }
        if self.angled_bottom && self.extent != Extent::All && self.style != Style::Drilled {
            required.push(&self.tip_angle);
        }
        if self.tapped {
            required.push(&self.thread);
            if !self.thread_options.full_length {
                required.push(self.thread_options.depth.as_ref().ok_or("hole_presets.invalid_parameters")?);
            }
        }
        if required.into_iter().any(|text| text.trim().is_empty())
            || self.thread_options.depth.as_ref().is_some_and(|text| !valid_text(text, MAX_EXPRESSION))
            || self.thread_options.ease_out.as_ref().is_some_and(|text| text.trim().is_empty() || !valid_text(text, MAX_EXPRESSION))
            || self.thread_options.drill.as_ref().is_some_and(|text| text.trim().is_empty() || !valid_text(text, MAX_EXPRESSION))
            || (self.thread_options.ease_out.is_some() && !(self.tapped && self.thread_options.modeled))
            || !["right", "left"].contains(&self.thread_options.handedness.as_str())
            || self.thread_options.unknown.len() > 8
            || self.thread_options.unknown.iter().any(|(key, value)| {
                key.is_empty()
                    || !valid_text(key, 64)
                    || match value {
                        serde_json::Value::String(text) => !valid_text(text, MAX_EXPRESSION),
                        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Null => false,
                        _ => true,
                    }
            })
        {
            return Err("hole_presets.invalid_parameters");
        }
        Ok(())
    }

    fn capture(d: &Dialog, name: &str, doc: &Document) -> Result<Self, &'static str> {
        let Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } = &d.kind else {
            return Err("hole_presets.invalid_parameters");
        };
        if opts.tapped && opts.custom_thread.as_ref().is_some_and(|draft| draft.error.is_some()) {
            return Err("hole_presets.invalid_parameters");
        }
        let (values, _) = doc.param_values();
        let lookup = |name: &str| values.get(name).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{name}`")));
        let portable_length = |text: &str, active: bool| -> Result<String, &'static str> {
            if text.trim().is_empty() {
                return Ok(text.into());
            }
            let checked = expr::eval_with(text, &lookup).and_then(|value| {
                value.to_kind(expr::Kind::Length)?;
                Ok(value)
            });
            let value = match checked {
                Ok(value) => value,
                // Hidden fields may contain an unfinished edit. Keep bounded text for
                // later editing; only dimensions used by this style/extent must parse.
                Err(_) if !active => return Ok(text.into()),
                Err(_) => return Err("hole_presets.invalid_parameters"),
            };
            if value.unitless() {
                let unit = expr::length_unit_or_mm(&doc.units);
                // Keep the expression, not its evaluated number. Explicit mm matters too:
                // an unsuffixed value would otherwise acquire the destination design's unit.
                Ok(if expr::is_bare_number(text) { format!("{} {unit}", text.trim()) } else { format!("({text}) * 1 {unit}") })
            } else {
                Ok(text.into())
            }
        };
        let preset = Self {
            name: name.trim().into(),
            style: match *kind {
                0 if opts.angled => Style::Drilled,
                0 => Style::Simple,
                1 => Style::Counterbore,
                2 => Style::Countersink,
                _ => return Err("hole_presets.invalid_parameters"),
            },
            angled_bottom: opts.angled,
            extent: if d.inputs.iter().any(|input| input.label == "To Object") {
                Extent::ToObject
            } else if opts.all {
                Extent::All
            } else {
                Extent::Distance
            },
            diameter: portable_length(diameter, !opts.tapped)?,
            depth: portable_length(depth, !opts.all && !d.inputs.iter().any(|input| input.label == "To Object"))?,
            cb_diameter: portable_length(cb_diameter, *kind == 1)?,
            cb_depth: portable_length(cb_depth, *kind == 1)?,
            cs_diameter: portable_length(cs_diameter, *kind == 2)?,
            cs_angle: cs_angle.clone(),
            tip_angle: opts.tip_angle.clone(),
            tapped: opts.tapped,
            thread: opts.thread.clone(),
            thread_options: if opts.tapped {
                ThreadOptions {
                    handedness: if opts.hand == solidcraft_engine::doc::thread_forms::ThreadHand::Left { "left".into() } else { "right".into() },
                    full_length: opts.thread_full_depth,
                    depth: if opts.thread_full_depth { None } else { Some(portable_length(&opts.thread_depth, true)?) },
                    modeled: opts.modeled,
                    // The runout as the command takes it: a named one, or the fade's length.
                    ease_out: match opts.ease_out.param() {
                        Some(serde_json::Value::String(text)) if opts.modeled && named_runout(&text) => Some(text),
                        Some(_) if opts.modeled => Some(portable_length(&opts.ease_out.length, true)?),
                        _ => None,
                    },
                    drill: opts.drill.as_deref().filter(|d| !d.trim().is_empty()).map(|d| portable_length(d, true)).transpose()?,
                    unknown: BTreeMap::new(),
                }
            } else {
                ThreadOptions::default()
            },
        };
        preset.validate()?;
        Ok(preset)
    }

    fn same_parameters(&self, other: &Self) -> bool {
        let normalized = |preset: &Self| {
            let mut p = preset.clone();
            p.name.clear();
            p.angled_bottom |= p.style == Style::Drilled;
            if p.style == Style::Drilled {
                p.style = Style::Simple;
            }
            // Unknown options cannot be captured by this form and cannot be overwritten.
            p.thread_options.unknown.clear();
            if !p.tapped {
                p.thread_options = ThreadOptions::default();
            } else if p.thread_options.full_length {
                p.thread_options.depth = None;
            }
            p
        };
        normalized(self) == normalized(other)
    }

    fn validate_active(&self, doc: &Document) -> Result<(), &'static str> {
        let length = |text: &str| doc.eval(text, expr::Kind::Length).is_ok_and(|v| v.is_finite() && v > 0.0);
        let angle = |text: &str| doc.eval(text, expr::Kind::Angle).is_ok_and(|v| v.is_finite() && v > 0.0 && v < std::f64::consts::PI);
        let style_valid = match self.style {
            Style::Counterbore => length(&self.cb_diameter) && length(&self.cb_depth),
            Style::Countersink => length(&self.cs_diameter) && angle(&self.cs_angle),
            Style::Simple | Style::Drilled => true,
        };
        if !style_valid
            || (!self.tapped && !length(&self.diameter))
            || (self.extent == Extent::Distance && !length(&self.depth))
            || (self.extent != Extent::All && (self.angled_bottom || self.style == Style::Drilled) && !angle(&self.tip_angle))
            || (self.tapped && solidcraft_engine::doc::parse_tapped_thread(&self.thread).is_none())
            || self.thread_options.ease_out.as_deref().is_some_and(|text| !named_runout(text) && !length(text))
            || self.thread_options.drill.as_deref().is_some_and(|text| {
                !solidcraft_engine::doc::parse_tapped_thread(&self.thread)
                    .is_some_and(|t| doc.eval(text, expr::Kind::Length).is_ok_and(|v| t.check_drill(v).is_ok()))
            })
            || (self.tapped && !self.thread_options.full_length && self.thread_options.depth.as_deref().is_none_or(|text| !length(text)))
        {
            return Err("hole_presets.invalid_parameters");
        }
        Ok(())
    }

    fn recall(&self, d: &mut Dialog) -> Result<(), &'static str> {
        self.validate()?;
        if !self.thread_options.unknown.is_empty() {
            return Err("hole_presets.unsupported_options");
        }
        let Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } = &mut d.kind else {
            return Err("hole_presets.invalid_parameters");
        };
        diameter.clone_from(&self.diameter);
        depth.clone_from(&self.depth);
        *kind = match self.style {
            Style::Simple | Style::Drilled => 0,
            Style::Counterbore => 1,
            Style::Countersink => 2,
        };
        cb_diameter.clone_from(&self.cb_diameter);
        cb_depth.clone_from(&self.cb_depth);
        cs_diameter.clone_from(&self.cs_diameter);
        cs_angle.clone_from(&self.cs_angle);
        // Drilled is the legacy representation; recesses have an independent bottom choice.
        opts.angled = self.angled_bottom || self.style == Style::Drilled;
        opts.tip_angle.clone_from(&self.tip_angle);
        opts.all = self.extent == Extent::All;
        opts.tapped = self.tapped;
        opts.thread.clone_from(&self.thread);
        opts.custom_thread = None;
        opts.thread_full_depth = self.thread_options.full_length;
        opts.hand = if self.thread_options.handedness == "left" {
            solidcraft_engine::doc::thread_forms::ThreadHand::Left
        } else {
            solidcraft_engine::doc::thread_forms::ThreadHand::Right
        };
        opts.thread_depth = self.thread_options.depth.clone().unwrap_or_else(|| crate::dialogs::HoleOpts::default().thread_depth);
        opts.modeled = self.thread_options.modeled;
        opts.drill.clone_from(&self.thread_options.drill);
        opts.ease_out = match self.thread_options.ease_out.as_deref() {
            None => crate::dialogs_thread::Ease::default(),
            Some(text) if named_runout(text) => crate::dialogs_thread::Ease { fade: true, short: text == "short", ..Default::default() },
            Some(text) => crate::dialogs_thread::Ease { fade: true, length: text.into(), short: false },
        };
        // Existing placement picks stay with this dialog. To Object stores its mode only:
        // recalling it always asks for a fresh destination in the current design.
        crate::dialogs::preset_to_object(d, self.extent == Extent::ToObject);
        Ok(())
    }
}

/// Stored as one preference array. A malformed row cannot discard unrelated preferences
/// or valid rows. Bound the retained rows and all user-controlled strings.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Library(Vec<Preset>);

impl<'de> Deserialize<'de> for Library {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct LibraryVisitor;
        impl<'de> de::Visitor<'de> for LibraryVisitor {
            type Value = Library;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an array of personal Hole presets")
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Library, A::Error> {
                let mut library = Library::default();
                for _ in 0..MAX_PRESETS {
                    let Some(row) = seq.next_element::<serde_json::Value>()? else { return Ok(library) };
                    if let Ok(preset) = serde_json::from_value::<Preset>(row)
                        && preset.validate().is_ok()
                        && library.find(&preset.name).is_none()
                    {
                        library.0.push(preset);
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
    fn find(&self, name: &str) -> Option<usize> {
        let name = name.to_lowercase();
        self.0.iter().position(|preset| preset.name.to_lowercase() == name)
    }

    fn save(&mut self, d: &Dialog, name: &str, doc: &Document) -> Result<(), &'static str> {
        if self.find(name.trim()).is_some() {
            return Err("hole_presets.name_exists");
        }
        if self.0.len() >= MAX_PRESETS {
            return Err("hole_presets.full");
        }
        self.0.push(Preset::capture(d, name, doc)?);
        Ok(())
    }

    fn modified(&self, selected: &str, d: &Dialog, doc: &Document) -> bool {
        self.find(selected).and_then(|index| self.0.get(index)).is_some_and(|saved| match Preset::capture(d, &saved.name, doc) {
            Ok(current) => !current.same_parameters(saved),
            Err(_) => true,
        })
    }

    fn overwrite(&mut self, selected: &str, d: &Dialog, doc: &Document) -> Result<(), &'static str> {
        let index = self.find(selected).ok_or("hole_presets.missing")?;
        let saved = self.0.get(index).ok_or("hole_presets.missing")?;
        if !saved.thread_options.unknown.is_empty() {
            return Err("hole_presets.unsupported_overwrite");
        }
        let candidate = Preset::capture(d, &saved.name, doc)?;
        candidate.validate_active(doc)?;
        // Capture and all validation finish before the existing record is replaced.
        *self.0.get_mut(index).ok_or("hole_presets.missing")? = candidate;
        Ok(())
    }

    fn rename(&mut self, old: &str, new: &str) -> Result<(), &'static str> {
        let new = new.trim();
        if !valid_name(new) {
            return Err("hole_presets.invalid_name");
        }
        let index = self.find(old).ok_or("hole_presets.missing")?;
        if self.find(new).is_some_and(|other| other != index) {
            return Err("hole_presets.name_exists");
        }
        let preset = self.0.get_mut(index).ok_or("hole_presets.missing")?;
        preset.name = new.into();
        Ok(())
    }
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

/// One hook above the existing Hole form. Only the library is persisted in preferences;
/// selection, errors and the naming prompt belong to this dialog's temporary UI state.
pub(crate) fn rows(ui: &mut egui::Ui, d: &mut Dialog, library: &mut Library, doc: &Document, reset: bool) {
    if !matches!(d.kind, Kind::Hole { .. }) {
        return;
    }
    let id = egui::Id::new("personal_hole_presets_panel");
    let mut state = if reset { PanelState::default() } else { ui.ctx().data_mut(|data| data.get_temp::<PanelState>(id)).unwrap_or_default() };
    ui_kit::section(ui, "personal_hole_presets", "hole_presets.title", false, |ui| {
        ui.label(tr("hole_presets.scope"));
        crate::dialogs::form_row(ui, "hole_presets.preset", |ui| {
            let before = library.find(&state.selected).map_or(0, |index| index + 1);
            let mut selected = before;
            let dirty = library.modified(&state.selected, d, doc);
            let display = library.find(&state.selected).and_then(|index| library.0.get(index)).map_or_else(
                || tr("hole_presets.select").to_string(),
                |preset| {
                    if dirty { format!("* {}", preset.name) } else { preset.name.clone() }
                },
            );
            let height = ui_kit::row_height();
            let width = (ui.available_width() - height - ui.spacing().item_spacing.x).max(1.0);
            ui.allocate_ui_with_layout(egui::vec2(width, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let response = ui_kit::dropdown_menu(ui, "personal_hole_preset_choice", &display, |ui| {
                    let placeholder = ui_kit::list_picker(ui, "hole_presets_select", tr("hole_presets.select"), selected == 0);
                    if placeholder.clicked() {
                        selected = 0;
                    }
                    crate::scenario::publish_handle(&format!("dialog:hole_presets.preset:{}", tr("hole_presets.select")), placeholder.rect.center());
                    for (index, preset) in library.0.iter().enumerate() {
                        let option = ui_kit::list_picker(ui, &format!("hole_preset_{index}"), &preset.name, selected == index + 1);
                        if option.clicked() {
                            selected = index + 1;
                        }
                        crate::scenario::publish_handle(&format!("dialog:hole_presets.preset:{}", preset.name), option.rect.center());
                    }
                });
                response.widget_info(|| {
                    let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), tr("hole_presets.preset"));
                    info.current_text_value = Some(display.clone());
                    info
                });
            });
            if selected != before {
                let preset = selected.checked_sub(1).and_then(|index| library.0.get(index));
                state.selected = preset.map_or_else(String::new, |preset| preset.name.clone());
                state.error = preset.and_then(|preset| preset.recall(d).err());
                state.prompt = None;
                state.confirming_delete = false;
            }
            let modified = library.modified(&state.selected, d, doc);
            ui.add_enabled_ui(modified, |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(height, height), egui::Sense::hover());
                let save = ui_kit::icon_button_at(ui, rect, "save", tr("hole_presets.overwrite"));
                crate::scenario::publish_target("dialog:hole_presets:overwrite", save.rect);
                if save.clicked() {
                    state.error = library.overwrite(&state.selected, d, doc).err();
                    state.confirming_delete = false;
                }
            });
        });
        let chosen = library.find(&state.selected);
        ui.horizontal_wrapped(|ui| {
            if ui_kit::button(ui, tr("hole_presets.save"), Button::Normal).clicked() {
                state.prompt = Some(NamePrompt { name: String::new(), renaming: false });
                state.error = None;
                state.confirming_delete = false;
            }
            if ui_kit::button(ui, tr("hole_presets.rename"), ui_kit::Button::Enabled(chosen.is_some())).clicked() {
                state.prompt = Some(NamePrompt { name: state.selected.clone(), renaming: true });
                state.error = None;
                state.confirming_delete = false;
            }
            if ui_kit::button(ui, tr("hole_presets.delete"), ui_kit::Button::Enabled(chosen.is_some())).clicked() {
                state.confirming_delete = true;
                state.error = None;
            }
        });
        if state.confirming_delete {
            ui.label(tr("hole_presets.delete_prompt"));
            ui.horizontal_wrapped(|ui| {
                if ui_kit::button(ui, tr("hole_presets.confirm_delete"), Button::Normal).clicked() {
                    if let Some(index) = library.find(&state.selected) {
                        library.0.remove(index);
                    }
                    state = PanelState::default();
                }
                if ui_kit::button(ui, tr("ui.cancel"), Button::Normal).clicked() {
                    state.confirming_delete = false;
                }
            });
        }
        if state.prompt.is_none()
            && let Some(error) = state.error
        {
            ui_kit::text(ui, tr(error), Text::Report("hole_presets_error", true));
        }
    });
    ui.ctx().data_mut(|data| data.insert_temp(id, state));
}

/// Global shortcuts run before dialogs. A pending name modal owns their keyboard even
/// when Escape has already blurred its text field. Stale state cannot block other tools.
pub(crate) fn name_prompt_open(ctx: &egui::Context, dialog: Option<&Dialog>) -> bool {
    dialog.is_some_and(|d| matches!(&d.kind, Kind::Hole { .. }))
        && ctx.data_mut(|data| data.get_temp::<PanelState>(egui::Id::new("personal_hole_presets_panel"))).is_some_and(|state| state.prompt.is_some())
}

/// Draw after the Hole card so the modal owns input and its footer handles. `true` also
/// covers the confirmation frame: Enter/Cancel must never reach the underlying Hole.
pub(crate) fn name_prompt(ctx: &egui::Context, d: &mut Dialog, library: &mut Library, doc: &Document) -> bool {
    if !matches!(d.kind, Kind::Hole { .. }) {
        return false;
    }
    let id = egui::Id::new("personal_hole_presets_panel");
    let mut state = ctx.data_mut(|data| data.get_temp::<PanelState>(id)).unwrap_or_default();
    let Some(mut prompt) = state.prompt.take() else { return false };
    let title = tr(if prompt.renaming { "hole_presets.rename" } else { "hole_presets.save" });
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("personal_hole_preset_name"), "save", title)
        .actions(if prompt.renaming { tr("hole_presets.rename") } else { crate::i18n::tr_label("Save") }, "Cancel", None)
        .show(ctx, true, |ui| {
            ui_kit::row(ui, "hole_presets.name", |ui| ui_kit::focused_text_field(ui, &mut prompt.name, tr("hole_presets.name_hint")));
            // Retain over-limit names for a clear validation error, but bound UI storage too.
            if prompt.name.len() > MAX_EXPRESSION {
                let end = prompt.name.char_indices().map(|(index, _)| index).take_while(|index| *index <= MAX_EXPRESSION).last().unwrap_or(0);
                prompt.name.truncate(end);
            }
            if let Some(error) = state.error {
                ui_kit::text(ui, tr(error), Text::Report("hole_presets_name_error", true));
            }
        });
    ctx.input_mut(|input| {
        input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
    });
    if closed || action == ui_kit::Action::Cancel {
        state.error = None;
    } else if action == ui_kit::Action::Ok {
        state.error = if prompt.renaming { library.rename(&state.selected, &prompt.name).err() } else { library.save(d, &prompt.name, doc).err() };
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
mod tests {
    use super::*;

    #[test]
    fn hole_presets_recall_preserves_current_reference_distances() {
        let harness = crate::scenario::Harness::new();
        let mut dialog = Dialog::for_command(&harness.app, "solid.hole").unwrap();
        let Kind::Hole { opts, .. } = &mut dialog.kind else { panic!("expected Hole") };
        opts.refs.distance = ["10 mm".into(), "12 mm".into()];
        let preset = Preset::capture(&dialog, "Portable", &Document::default()).unwrap();
        let saved = serde_json::to_value(&preset).unwrap();
        assert!(saved.get("references").is_none() && saved.get("refs").is_none());
        let Kind::Hole { opts, .. } = &mut dialog.kind else { panic!("expected Hole") };
        opts.refs.distance = ["17 mm".into(), "18 mm".into()];
        let references = opts.refs.clone();
        let inputs = dialog.inputs.clone();
        preset.recall(&mut dialog).unwrap();
        let Kind::Hole { opts, .. } = &dialog.kind else { panic!("expected Hole") };
        assert_eq!(*opts.refs, *references);
        assert_eq!(dialog.inputs, inputs);
    }

    /// A tapped preset keeps the thread's form and options: an Rc and an NPT size, modelled,
    /// with a typed fade length or ISO 3508's short runout, and a square end; saved, read back
    /// and recalled into a fresh dialog, each comes back as it was captured.
    #[test]
    fn hole_presets_round_trip_modelled_taper_threads_and_their_ends() {
        use crate::dialogs_thread::Ease;
        use solidcraft_engine::doc::thread_forms::ThreadHand;
        let harness = crate::scenario::Harness::new();
        let doc = Document::default();
        for (thread, modeled, ease, saved) in [
            ("Rc 1/2", true, Ease { fade: true, length: "1.5 mm".into(), short: false }, Some("1.5 mm")),
            ("1/2-14 NPT", true, Ease { fade: true, length: String::new(), short: true }, Some("short")),
            ("Rc 3/4", true, Ease::default(), None),
            ("G 1/4", false, Ease::default(), None),
        ] {
            let mut dialog = Dialog::for_command(&harness.app, "solid.hole").unwrap();
            let Kind::Hole { opts, .. } = &mut dialog.kind else { panic!("expected Hole") };
            opts.tapped = true;
            opts.thread = thread.into();
            opts.hand = ThreadHand::Left;
            opts.thread_full_depth = false;
            opts.thread_depth = "8 mm".into();
            opts.modeled = modeled;
            opts.ease_out = ease.clone();
            // An Rc 1/2 drilled 17.8 mm, inside its 17.6 to 18.48 mm.
            opts.drill = (thread == "Rc 1/2").then(|| "17.8 mm".to_string());
            let captured = opts.clone();
            let preset = Preset::capture(&dialog, "Pipe", &doc).unwrap();
            let row = serde_json::to_value(&preset).unwrap();
            assert_eq!(row["thread_options"].get("ease_out").and_then(|v| v.as_str()), saved, "{thread}: {row}");
            assert_eq!(row["thread_options"].get("modeled").is_some(), modeled, "{thread}: {row}");
            let read: Preset = serde_json::from_value(row).unwrap();
            assert_eq!(read, preset);
            read.validate_active(&doc).unwrap_or_else(|e| panic!("{thread}: {e}"));
            let mut fresh = Dialog::for_command(&harness.app, "solid.hole").unwrap();
            read.recall(&mut fresh).unwrap();
            let Kind::Hole { opts, .. } = &fresh.kind else { panic!("expected Hole") };
            assert_eq!(
                (&opts.thread, opts.hand, opts.thread_full_depth, &opts.thread_depth, opts.modeled, &opts.ease_out, &opts.drill),
                (&captured.thread, captured.hand, false, &captured.thread_depth, modeled, &ease, &captured.drill),
                "{thread}"
            );
        }
        // A runout without a modelled thread is no preset's.
        let mut dialog = Dialog::for_command(&harness.app, "solid.hole").unwrap();
        let Kind::Hole { opts, .. } = &mut dialog.kind else { panic!("expected Hole") };
        opts.tapped = true;
        opts.thread = "M8".into();
        let mut preset = Preset::capture(&dialog, "Square", &doc).unwrap();
        preset.thread_options.ease_out = Some("standard".into());
        assert!(preset.validate().is_err());
        // A drill outside the size's range is no preset to recall.
        preset.thread_options.ease_out = None;
        preset.thread_options.drill = Some("7.5 mm".into());
        assert!(preset.validate().is_ok() && preset.validate_active(&doc).is_err(), "M8 takes 6.647 to 7.14 mm");
    }
}
