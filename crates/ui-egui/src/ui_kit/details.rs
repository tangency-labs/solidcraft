//! Description-driven details content. No tool, document or command knowledge lives here.
use crate::ui_kit::BackingTooltipExt as _;
use crate::{fonts::RichTextExt as _, scenario, theme::Tokens};
use egui::{RichText, Ui};

const FIELD_FOCUS: &str = "details_field_focus";
const CONTROL_FOCUS: &str = "details_control_focus";
const OWNED_POPUPS: &str = "details_owned_popups";
pub(crate) fn dismiss_details_popups(ctx: &egui::Context) {
    let popups = ctx.data(|data| data.get_temp::<Vec<egui::Id>>(egui::Id::new(OWNED_POPUPS))).unwrap_or_default();
    for popup in popups {
        egui::Popup::close_id(ctx, popup);
    }
}
fn remember_control(ui: &Ui, id: egui::Id) {
    if !ui.is_visible() || !ui.is_enabled() {
        return;
    }
    ui.ctx().data_mut(|data| {
        let key = egui::Id::new(CONTROL_FOCUS);
        let mut ids = data.get_temp::<Vec<egui::Id>>(key).unwrap_or_default();
        ids.push(id);
        data.insert_temp(key, ids);
    });
}
pub(crate) fn details_control_owns_focus(ctx: &egui::Context) -> bool {
    let ids = ctx.data(|data| data.get_temp::<Vec<egui::Id>>(egui::Id::new(CONTROL_FOCUS))).unwrap_or_default();
    ctx.memory(|memory| memory.focused().is_some_and(|id| ids.contains(&id)))
}
pub(crate) fn details_field_owns_focus(ctx: &egui::Context) -> bool {
    let fields = ctx.data(|data| data.get_temp::<Vec<(egui::Id, egui::Rect)>>(egui::Id::new(FIELD_FOCUS))).unwrap_or_default();
    let focused = ctx.memory(|memory| memory.focused());
    focused.is_some_and(|id| {
        fields.iter().any(|(field, rect)| *field == id || ctx.read_response(id).is_some_and(|response| rect.contains(response.rect.center())))
    })
}
pub(crate) fn details_field_owned_escape(ctx: &egui::Context) -> bool {
    if details_field_owns_focus(ctx) {
        return true;
    }
    let fields = ctx.data(|data| data.get_temp::<Vec<(egui::Id, egui::Rect)>>(egui::Id::new(FIELD_FOCUS))).unwrap_or_default();
    // egui blurs native value editors before global shortcuts see Escape. The
    // preceding visible frame still owns that press, just as a text editor does.
    ctx.memory(|memory| memory.focused().is_none() && fields.iter().any(|(id, _)| memory.had_focus_last_frame(*id)))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DetailValue {
    Number { value: f64, min: f64, max: f64, suffix: String },
    Length { value: f64, units: String },
    Angle(f64),
    Slider { value: f64, min: f64, max: f64 },
    Axis { value: [f64; 3], current: String },
    Check(bool),
    Choice { selected: usize, options: Vec<String> },
}

pub(crate) struct DetailSetting {
    pub id: String,
    pub label: String,
    pub value: DetailValue,
    pub enabled: bool,
}

pub(crate) struct DetailKey {
    pub colour: egui::Color32,
    pub pattern: bool,
    pub label: String,
    pub value: String,
    pub numeric: bool,
    pub pattern_colour: Option<egui::Color32>,
}

pub(crate) struct DetailFinding {
    pub id: String,
    pub label: String,
    pub value: String,
    pub warning: bool,
    pub target: bool,
}

/// All blocks are optional. Strings and numeric values are current caller-owned data.
#[derive(Default)]
pub(crate) struct Details {
    pub caption: Option<String>,
    pub settings: Vec<DetailSetting>,
    pub key: Vec<DetailKey>,
    pub findings_title: String,
    pub findings_count: Option<String>,
    pub findings: Vec<DetailFinding>,
    pub readout: Vec<(String, String)>,
    pub actions: Vec<(String, String)>,
}

#[derive(Debug, PartialEq)]
pub(crate) enum DetailEvent {
    Change(String, DetailValue),
    Target(String),
    Action(String),
}

fn heading(ui: &mut Ui, label: &str) {
    heading_value(ui, label, None);
}
fn heading_value(ui: &mut Ui, label: &str, value: Option<&str>) {
    let t = Tokens::get();
    super::divider(ui, label);
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).semibold().size(t.font_small).color(t.text_dim));
        if let Some(value) = value {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(value).monospace().size(t.font_small).color(t.text_dim));
            });
        }
    });
}

/// Shared semantic swatch: pattern supplements the analysis colour in every vision mode.
pub(crate) fn patterned_swatch(ui: &mut Ui, colour: egui::Color32, pattern: bool, pattern_colour: Option<egui::Color32>) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(t.space_12, t.space_12), egui::Sense::hover());
    ui.painter().rect_filled(rect, t.radius_sm, colour);
    if pattern {
        let painter = ui.painter().with_clip_rect(rect);
        for offset in [-t.space_8, -t.space_4, 0.0, t.space_4, t.space_8] {
            let from = rect.left_bottom() + egui::vec2(offset, 0.0);
            painter.line_segment([from, from + egui::vec2(rect.height(), -rect.height())], t.primary_border(pattern_colour.unwrap_or(t.text)));
        }
    }
}

fn setting(ui: &mut Ui, field: &DetailSetting, focus: bool) -> Option<DetailEvent> {
    let mut value = field.value.clone();
    let mut changed = false;
    super::row(ui, &field.label, |ui| {
        ui.add_enabled_ui(field.enabled, |ui| {
            ui.style_mut().drag_value_text_style = egui::TextStyle::Monospace;
            let response = match &mut value {
                DetailValue::Number { value, min, max, suffix } => Some(super::number_field(ui, value, *min..=*max, 0.01, suffix)),
                DetailValue::Length { value, units } => Some(super::length_field(ui, value, units, 6)),
                DetailValue::Angle(value) => Some(super::angle_field(ui, value)),
                DetailValue::Slider { value, min, max } => Some(super::slider(ui, value, *min..=*max, Default::default())),
                DetailValue::Check(value) => Some(super::checkbox(ui, "", value)),
                DetailValue::Axis { value, .. } => {
                    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
                    let mut selected = axes.iter().position(|axis| *value == *axis).unwrap_or(usize::MAX);
                    let wanted_focus = (focus && ui.is_visible()).then_some(if selected < 3 { selected } else { 0 });
                    for index in 0..3 {
                        remember_control(ui, ui.make_persistent_id(("segment", index)));
                    }
                    if super::segmented(
                        ui,
                        &mut selected,
                        &["X", "Y", "Z"],
                        super::Segments { focus: wanted_focus, compact: true, ..Default::default() },
                    ) && let Some(axis) = axes.get(selected)
                    {
                        *value = *axis;
                        changed = true;
                    }
                    None
                }
                DetailValue::Choice { selected, options } => {
                    let before = *selected;
                    let current = options.get(*selected).cloned().unwrap_or_default();
                    let response = super::dropdown_menu(ui, &field.id, &current, |ui| {
                        for (index, option) in options.iter().enumerate() {
                            let response = ui.selectable_value(selected, index, option);
                            remember_control(ui, response.id);
                        }
                    });
                    ui.ctx().data_mut(|data| {
                        let key = egui::Id::new(OWNED_POPUPS);
                        let mut popups = data.get_temp::<Vec<egui::Id>>(key).unwrap_or_default();
                        // ComboBox's native popup is keyed by the returned button response.
                        popups.push(response.id.with("popup"));
                        data.insert_temp(key, popups);
                    });
                    changed = *selected != before;
                    Some(response)
                }
            };
            if let Some(response) = response {
                scenario::publish_target(&format!("details:field:{}", field.id), response.rect);
                // Visibility reads Context::will_discard. Never re-enter Context while
                // holding its data write lock.
                let visible = ui.is_visible() && ui.is_enabled() && ui.is_rect_visible(response.rect);
                if focus {
                    ui.scroll_to_rect(response.rect, None);
                    if visible {
                        response.request_focus();
                    }
                }
                // A DragValue may focus its inner TextEdit rather than its outer
                // response. Retain only the actual native editor inside this field.
                let native_focus = ui.ctx().memory(|memory| memory.focused()).filter(|id| {
                    *id == response.id || ui.ctx().read_response(*id).is_some_and(|native| response.rect.contains(native.rect.center()))
                });
                ui.ctx().data_mut(|data| {
                    let key = egui::Id::new(FIELD_FOCUS);
                    let mut fields = data.get_temp::<Vec<(egui::Id, egui::Rect)>>(key).unwrap_or_default();
                    if visible {
                        fields.push((response.id, response.rect));
                        if let Some(id) = native_focus {
                            fields.push((id, response.rect));
                        }
                    }
                    data.insert_temp(key, fields);
                });
                changed |= response.changed();
            }
        });
    });
    if let DetailValue::Axis { value: vector, current } = &value
        && ![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].contains(vector)
    {
        let response = ui.add(egui::Label::new(RichText::new(current).monospace()).wrap()).backing_on_hover_text(current);
        scenario::publish_target(&format!("details:current:{}", field.id), response.rect);
    }
    changed.then(|| DetailEvent::Change(field.id.clone(), value))
}

/// Fixed caption and actions surround one bounded scrolling body. The six-row findings
/// preview expands inside that body; it never creates another popup or steals CAD input.
pub(crate) fn details(ui: &mut Ui, id: &str, description: &Details, max_height: f32, focus: Option<&str>) -> Vec<DetailEvent> {
    let t = Tokens::get();
    let mut events = Vec::new();
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(FIELD_FOCUS), Vec::<(egui::Id, egui::Rect)>::new()));
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(CONTROL_FOCUS), Vec::<egui::Id>::new()));
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new(OWNED_POPUPS), Vec::<egui::Id>::new()));
    ui.set_width((t.form_card_width + if description.findings.is_empty() { 0.0 } else { t.space_24 * 2.0 }).min(ui.available_width()));
    super::scope(ui, |ui| {
        let mut caption_height = 0.0;
        if let Some(caption) = &description.caption {
            let response = ui.add(egui::Label::new(RichText::new(caption).size(t.font_label).color(t.text_dim)).wrap());
            caption_height = response.rect.height() + t.space_8;
            scenario::publish_rect("details:caption", response.rect);
        }
        let footer_height = if description.actions.is_empty() { 0.0 } else { t.control_height + t.space_16 };
        let body = egui::ScrollArea::vertical()
            .id_salt(("details", id))
            .max_height((max_height - caption_height - footer_height - t.space_24).max(0.0))
            .min_scrolled_height(0.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if !description.settings.is_empty() {
                    heading(ui, crate::i18n::tr("ui.details_settings"));
                    super::settings_form(ui, t.form_label_width, |ui| {
                        for field in &description.settings {
                            ui.push_id(&field.id, |ui| {
                                if let Some(event) = setting(ui, field, focus == Some(field.id.as_str())) {
                                    events.push(event);
                                }
                            });
                        }
                    });
                }
                if !description.key.is_empty() {
                    heading(ui, crate::i18n::tr("ui.details_key"));
                    for row in &description.key {
                        ui.horizontal(|ui| {
                            patterned_swatch(ui, row.colour, row.pattern, row.pattern_colour);
                            let label = RichText::new(&row.label);
                            ui.label(if row.numeric { label.monospace() } else { label });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.label(RichText::new(&row.value).monospace());
                            });
                        });
                    }
                }
                if !description.findings.is_empty() {
                    heading_value(ui, &description.findings_title, description.findings_count.as_deref());
                    let expanded_id = ui.make_persistent_id((id, "all_findings"));
                    let expanded = ui.data(|data| data.get_temp::<bool>(expanded_id)).unwrap_or(false);
                    let columns = [super::Column::new("", 1.0).flex(), super::Column::new("", 1.0).flex(), super::Column::new("", 0.0).icon_only()];
                    super::table_custom(ui, id, &columns, "", |ui, table| {
                        for row in description.findings.iter().take(if expanded { usize::MAX } else { 6 }) {
                            table.row(ui, &row.id, false, |cells| {
                                cells.cell(|ui| {
                                    ui.horizontal(|ui| {
                                        if row.warning {
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(t.space_16, t.space_16), egui::Sense::hover());
                                            crate::icons::paint_icon(ui.painter(), rect, "warning", 1.0);
                                        }
                                        ui.label(&row.label).backing_on_hover_text(&row.label);
                                    });
                                });
                                cells.cell(|ui| {
                                    ui.label(RichText::new(&row.value).monospace().size(t.font_small)).backing_on_hover_text(&row.value);
                                });
                                cells.cell(|ui| {
                                    if row.target {
                                        let (rect, _) = ui.allocate_exact_size(egui::vec2(t.control_height, t.control_height), egui::Sense::hover());
                                        let response = ui
                                            .push_id(&row.id, |ui| super::icon_button_at(ui, rect, "fit", crate::i18n::tr("ui.details_target")))
                                            .inner;
                                        remember_control(ui, response.id);
                                        scenario::publish_target(&format!("details:target:{}", row.id), response.rect);
                                        if response.clicked() {
                                            events.push(DetailEvent::Target(row.id.clone()));
                                        }
                                    }
                                });
                            });
                        }
                    });
                    scenario::publish_count(
                        "details:findings_visible",
                        if expanded { description.findings.len() } else { description.findings.len().min(6) } as f64,
                    );
                    if !expanded && description.findings.len() > 6 {
                        let text = crate::i18n::format("ui.details_show_all", &[("count", &description.findings.len().to_string())]);
                        let response = super::button(ui, text, super::Button::Ghost);
                        remember_control(ui, response.id);
                        scenario::publish_target("details:show_all", response.rect);
                        if response.clicked() {
                            ui.data_mut(|data| data.insert_temp(expanded_id, true));
                        }
                    }
                } else if !description.readout.is_empty() {
                    heading(ui, crate::i18n::tr("ui.details_readout"));
                    let expanded_id = ui.make_persistent_id((id, "all_readout"));
                    let expanded = ui.data(|data| data.get_temp::<bool>(expanded_id)).unwrap_or(false);
                    let columns = [super::Column::new("", 1.0).flex(), super::Column::new("", 1.0).flex()];
                    super::table_custom(ui, id, &columns, "", |ui, table| {
                        for (index, (label, value)) in description.readout.iter().take(if expanded { usize::MAX } else { 6 }).enumerate() {
                            table.row(ui, &index.to_string(), false, |row| {
                                row.cell(|ui| {
                                    ui.label(label);
                                });
                                row.cell(|ui| {
                                    ui.label(RichText::new(value).monospace());
                                });
                            });
                        }
                    });
                    scenario::publish_count(
                        "details:readout_visible",
                        if expanded { description.readout.len() } else { description.readout.len().min(6) } as f64,
                    );
                    if !expanded && description.readout.len() > 6 {
                        let text = crate::i18n::format("ui.details_show_all", &[("count", &description.readout.len().to_string())]);
                        let response = super::button(ui, text, super::Button::Ghost);
                        remember_control(ui, response.id);
                        scenario::publish_target("details:show_all", response.rect);
                        if response.clicked() {
                            ui.data_mut(|data| data.insert_temp(expanded_id, true));
                        }
                    }
                }
            });
        scenario::publish_rect("details:body", body.inner_rect);
        if !description.actions.is_empty() {
            super::divider(ui, "details_actions");
            let actions = ui.horizontal(|ui| {
                for (id, label) in description.actions.iter().take(2) {
                    let response = super::text_button(ui, label, true);
                    remember_control(ui, response.id);
                    scenario::publish_target(&format!("details:action:{id}"), response.rect);
                    if response.clicked() {
                        events.push(DetailEvent::Action(id.clone()));
                    }
                }
            });
            scenario::publish_rect("details:actions", actions.response.rect);
        }
    });
    events
}
