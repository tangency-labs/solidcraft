//! Choice elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use super::{
    fields::dropdown,
    layout::{label, section},
    metrics::*,
    text::paint_text,
};
use crate::ui_kit::{Container, Surface};
use crate::{scenario, theme::Tokens};
use egui::{Ui, vec2};

/// Text segments retain mixed states and optional compact clipping instead of dropdown fallback.
#[derive(Default)]
pub(crate) struct Segments<'a> {
    pub states: &'a [bool],
    pub compact: bool,
    pub primary: bool,
    pub intrinsic: bool,
    pub proportional: bool,
}

pub(crate) fn segmented(ui: &mut Ui, selected: &mut usize, options: &[&str], config: Segments<'_>) -> bool {
    let Segments { states, compact, primary, intrinsic, proportional } = config;
    let t = Tokens::get();
    let segment_width = dimension(ui.available_width()) / options.len().max(1) as f32;
    let widest = options
        .iter()
        .map(|option| {
            ui.painter().layout_no_wrap(crate::i18n::tr_label(option).to_string(), crate::fonts::semibold(t.font_label), t.form_ink).size().x
        })
        .fold(0.0, f32::max);
    // Judged on the whole row, which is what a scrollbar narrows, not on one segment.
    if !intrinsic
        && !proportional
        && !compact
        && !super::fits_steadily(ui, label(ui), (widest + 2.0 * t.space_8) * options.len() as f32, segment_width * options.len() as f32)
    {
        let before = *selected;
        let id = format!("segments_fallback_{}", label(ui));
        dropdown(ui, &id, selected, options);
        return *selected != before;
    }
    let natural: Vec<_> = options
        .iter()
        .map(|option| {
            ui.painter().layout_no_wrap((*option).to_owned(), crate::fonts::semibold(t.font_label), t.form_ink).size().x.ceil() + t.space_16
        })
        .collect();
    let total = natural.iter().sum::<f32>();
    let desired = if intrinsic { total.min(dimension(ui.available_width())) } else { dimension(ui.available_width()) };
    let (rect, _) = ui.allocate_exact_size(vec2(desired, row_height()), egui::Sense::hover());
    ui.painter().rect_filled(rect, t.control_radius, t.control_fill);
    let equal_width = rect.width() / options.len().max(1) as f32;
    let mut clicked = false;
    let mut x = rect.left();
    for (index, option) in options.iter().enumerate() {
        let width =
            if (intrinsic || proportional) && total > 0.0 { natural.get(index).copied().unwrap_or(0.0) * rect.width() / total } else { equal_width };
        let segment = egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(width, rect.height()));
        x += width;
        let response = ui.interact(segment, ui.make_persistent_id(("segment", index)), egui::Sense::click());
        let on = states.get(index).copied().unwrap_or(*selected == index);
        let rounding = egui::CornerRadius {
            nw: if index == 0 { t.control_radius } else { 0 },
            sw: if index == 0 { t.control_radius } else { 0 },
            ne: if index + 1 == options.len() { t.control_radius } else { 0 },
            se: if index + 1 == options.len() { t.control_radius } else { 0 },
        };
        if on || response.hovered() {
            ui.painter().rect_filled(
                segment,
                rounding,
                if on && primary {
                    t.form_primary
                } else if on {
                    t.selected_bg
                } else {
                    t.hover
                },
            );
        }
        if index > 0 {
            ui.painter().vline(segment.left(), segment.y_range(), t.control_border_stroke(t.control_border));
        }
        let font = if on { crate::fonts::semibold(t.font_label) } else { egui::FontId::proportional(t.font_label) };
        let galley = ui.painter().layout_no_wrap((*option).to_string(), font.clone(), t.form_ink);
        let text_rect =
            egui::Rect::from_min_size(egui::pos2(segment.center().x - galley.size().x / 2.0, segment.top()), vec2(galley.size().x, segment.height()));
        let text_rect = if compact { text_rect.intersect(segment.shrink2(vec2(t.space_4, 0.0))) } else { text_rect };
        paint_text(
            ui,
            text_rect,
            option,
            font,
            if on && primary {
                t.on_accent
            } else if on {
                t.selected_ink
            } else {
                t.form_ink
            },
            false,
        );
        t.button_bevel_with_radius(ui, &response, on, rounding);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, *option));
        scenario::publish_handle(&format!("dialog:{}:{option}", label(ui)), segment.center());
        if response.on_hover_text(*option).clicked() {
            *selected = index;
            clicked = true;
        }
    }
    if t.borders.bevel.is_none() {
        ui.painter().rect_stroke(rect, t.control_radius, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    }
    scenario::publish_target_as(&format!("dialog:{}", label(ui)), &format!("dialog:segments:{}", label(ui)), rect);
    clicked
}

/// One segment of [`segmented_icons`]: a stable key (also the accessible fallback text), the icon
/// master to draw and the hover text. The icon's colours come from the palette (its roles and the
/// accent assigned to its id in `palette.toml`), not from the caller.
pub(crate) struct IconSegment {
    pub key: &'static str,
    pub icon: &'static str,
    pub tip: String,
}

/// A row of equal icon segments in the segmented control's frame. `states` says which segments are on
/// (several may be, for a mixed selection); without it `selected` is. True when a segment was clicked.
/// Equal icon choices with mixed state. Tokens: `segmented tokens, space_4/12, icon/fill, accent; icon palette`.
pub(crate) fn segmented_icons(ui: &mut Ui, selected: &mut usize, options: &[IconSegment], states: &[bool]) -> bool {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), row_height()), egui::Sense::hover());
    ui.painter().rect_filled(rect, t.control_radius, t.control_fill);
    let width = rect.width() / options.len().max(1) as f32;
    let mut clicked = false;
    for (index, option) in options.iter().enumerate() {
        let segment = egui::Rect::from_min_size(rect.min + vec2(index as f32 * width, 0.0), vec2(width, rect.height()));
        let response = ui.interact(segment, ui.make_persistent_id(("icon_segment", index)), egui::Sense::click());
        let on = states.get(index).copied().unwrap_or(*selected == index);
        let rounding = egui::CornerRadius {
            nw: if index == 0 { t.control_radius } else { 0 },
            sw: if index == 0 { t.control_radius } else { 0 },
            ne: if index + 1 == options.len() { t.control_radius } else { 0 },
            se: if index + 1 == options.len() { t.control_radius } else { 0 },
        };
        if on || response.hovered() {
            ui.painter().rect_filled(segment, rounding, if on { t.selected_bg } else { t.hover });
        }
        if index > 0 {
            ui.painter().vline(segment.left(), segment.y_range(), t.control_border_stroke(t.control_border));
        }
        // A square icon area inside the segment: dash gaps and dots need the room to read apart.
        let side = (segment.width().min(segment.height()) - t.space_4).max(t.space_12);
        let icon_rect = egui::Rect::from_center_size(segment.center(), vec2(side, side));
        crate::icons::paint_icon(ui.painter(), icon_rect, option.icon, f32::from(t.icon.a()) / 255.0);
        t.button_bevel_with_radius(ui, &response, on, rounding);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, option.key));
        scenario::publish_handle(&format!("dialog:{}:{}", label(ui), option.key), segment.center());
        let response = response.on_hover_text(&option.tip);
        if response.clicked() {
            *selected = index;
            clicked = true;
        }
    }
    if t.borders.bevel.is_none() {
        ui.painter().rect_stroke(rect, t.control_radius, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    }
    scenario::publish_target_as(&format!("dialog:{}", label(ui)), &format!("dialog:segments:{}", label(ui)), rect);
    clicked
}

/// Compact checkbox options in bounded columns. Tokens: `form_checkbox_height, control_fill/border, radius_sm, accent, on_accent, font_body, text, space_2/3/6/8/14`.
pub(crate) fn checkbox_grid(ui: &mut Ui, items: &mut [(&str, &mut bool)], columns: usize) {
    let t = Tokens::get();
    let columns = columns.clamp(1, items.len().max(1));
    let width = ((dimension(ui.available_width()) - t.space_8 * columns.saturating_sub(1) as f32) / columns as f32).max(0.0);
    for chunk in items.chunks_mut(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(t.space_8, 0.0);
            for (label, value) in chunk {
                let (rect, mut response) = ui.allocate_exact_size(vec2(width, t.form_checkbox_height), egui::Sense::click());
                if response.clicked() {
                    **value = !**value;
                    response.mark_changed();
                }
                let square = super::checks::box_at(ui, egui::pos2(rect.left() + t.space_14 / 2.0, rect.center().y));
                super::checks::paint_box(ui, square, &response, **value, false);
                let translated = crate::i18n::tr_label(label);
                let text = if translated == "Tangent Chain" { "Tangent chain" } else { translated };
                let text_rect = egui::Rect::from_min_max(egui::pos2((square.right() + t.space_6).min(rect.right()), rect.top()), rect.max);
                paint_text(ui, text_rect, text, egui::FontId::proportional(t.font_body), t.form_ink, false);
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), **value, translated));
                scenario::publish_target_as(&format!("dialog:{label}"), &format!("dialog:checkbox:{label}"), rect);
            }
        });
    }
}

/// Shared expanded options band for feature dialogs.
/// Expanded two-column Options disclosure. Tokens: `section and checkbox_grid tokens`.
pub(crate) fn options(ui: &mut Ui, id: &str, items: &mut [(&str, &mut bool)]) {
    section(ui, id, "Options", true, |ui| checkbox_grid(ui, items, 2));
}

/// Entire row is the checkbox hit target, including its label on the left.
/// Full-row target with label left and box right; returns clicked. Tokens: `control_height, control_radius, hover, radius_sm, accent, control_fill/border, on_accent, font_label, text, space_2/3/6/14`.
pub(crate) fn checkbox_row(ui: &mut egui::Ui, key: &str, label: &str, on: bool) -> bool {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height), egui::Sense::click());
    let square = super::checks::box_at(ui, egui::pos2(rect.right() - t.space_14 / 2.0, rect.center().y));
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    super::checks::paint_box(ui, square, &response, on, false);
    let label = crate::i18n::tr_label(label);
    let painter = ui
        .painter()
        .with_clip_rect(ui.clip_rect().intersect(egui::Rect::from_min_max(rect.min, egui::pos2(square.left() - t.space_6, rect.bottom()))));
    painter.text(rect.left_center(), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(t.font_label), t.form_ink);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), on, label));
    crate::scenario::publish_target(&format!("sketch_palette:toggle:{key}"), rect);
    response.on_hover_text(label).clicked()
}

/// Compact bold/italic toggles for text. Tokens: scope font/control tokens.
/// The glyphs keep the familiar B/I treatment; accessible names use translated style names.
pub(crate) fn text_emphasis(ui: &mut Ui, bold: &mut bool, italic: &mut bool) {
    use crate::fonts::RichTextExt as _;
    for (key, text, value, id) in [
        ("Bold", egui::RichText::new("B").semibold(), bold, "sketch_text.bold"),
        ("Italic", egui::RichText::new("I").italics(), italic, "sketch_text.italic"),
    ] {
        let response = ui.toggle_value(value, text).on_hover_text(crate::i18n::tr(id));
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), *value, crate::i18n::tr(id)));
        scenario::publish_target_as(&format!("dialog:{key}"), &format!("dialog:emphasis:{key}"), response.rect);
    }
}

/// A command variant supplied by the panel; the kit only draws and returns its key.
pub(crate) struct ToolChoice<'a> {
    pub key: &'a str,
    pub icon: &'a str,
    pub label: &'a str,
    pub enabled: bool,
}

/// Wrapped creation-mode icon choices. Tokens: sketch_mode_*, sketch_accent, control_*, icon, spacing.
/// Wrapped command-variant icons, supplied with enabled state. Tokens: `sketch_mode_size/radius/border/tint, sketch_accent, control_fill/border, icon/fill, space_4/6`.
pub(crate) fn tool_choices<'a>(ui: &mut Ui, label: &str, current: &str, choices: &[ToolChoice<'a>]) -> Option<&'a str> {
    let t = Tokens::get();
    let mut start = None;
    ui.label(label);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(t.space_4, t.space_4);
        for choice in choices {
            let on = choice.key == current;
            let enabled = choice.enabled;
            let fill = if on { t.control_fill.lerp_to_gamma(t.sketch_accent, t.sketch_mode_tint) } else { t.control_fill };
            let response = ui.add_enabled(
                enabled,
                egui::Button::new("")
                    .fill(fill)
                    .stroke(if on { egui::Stroke::new(t.sketch_mode_border, t.sketch_accent) } else { t.control_border_stroke(t.control_border) })
                    .corner_radius(t.sketch_mode_radius)
                    .min_size(vec2(t.sketch_mode_size, t.sketch_mode_size)),
            );
            crate::icons::paint_icon(ui.painter(), response.rect.shrink(t.space_6), choice.icon, f32::from(t.icon.a()) / 255.0);
            t.button_bevel(ui, &response, on);
            scenario::publish_target(&format!("sketch_palette:feature:{}", choice.key), response.rect);
            response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, on, choice.label));
            if response.on_hover_text(choice.label).clicked() && enabled {
                start = Some(choice.key);
            }
        }
    });
    start
}

/// Independent selected icon action, used for pattern direction/centering.
/// Tokens: control_height, space_3/4/24, icon/fill, accent; inherited control surface tokens.
pub(crate) fn toggle_icon(ui: &mut Ui, row: &str, label: &str, selected: &mut bool, icon: &str) -> egui::Response {
    // Tab moves between value fields; Enter after a value edit applies the dialog.
    let t = Tokens::get();
    let response =
        ui.add(egui::Button::new("").selected(*selected).min_size(vec2(t.space_24 + t.space_4, t.control_height)).sense(egui::Sense::CLICK));
    crate::icons::paint_icon(ui.painter(), response.rect.shrink(t.space_3), icon, f32::from(t.icon.a()) / 255.0);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, *selected, crate::i18n::tr_label(label)));
    t.button_bevel(ui, &response, *selected);
    crate::scenario::publish_handle(&format!("dialog:{row}:{label}"), response.rect.center());
    if response.clone().on_hover_text(crate::i18n::tr_label(label)).clicked() {
        *selected = !*selected;
    }
    scenario::publish_rect(&format!("dialog:{row}:{label}"), response.rect);
    response
}

#[cfg(test)]
#[path = "choice_tests.rs"]
mod tests;

/// Selectable list item retaining checked state and full text as an accessible label.
/// Tokens: control_height and scope selection/control/font tokens.
pub(crate) fn list_picker(ui: &mut Ui, key: &str, text: &str, selected: bool) -> egui::Response {
    let response =
        ui.add_sized(egui::vec2(super::metrics::dimension(ui.available_width()), super::row_height()), egui::Button::selectable(selected, text));
    crate::scenario::publish_target(&format!("dialog:list:{key}"), response.rect);
    response
}

/// Intrinsic-width selection chip for horizontal lists, sharing list-picker styling.
pub(crate) fn choice_chip(ui: &mut Ui, key: &str, text: &str, selected: bool) -> egui::Response {
    let t = Tokens::get();
    let width = (ui.painter().layout_no_wrap(text.to_owned(), crate::fonts::semibold(t.font_label), t.text).size().x + t.space_24)
        .clamp(t.control_height * 2.0, t.form_card_width);
    let response = ui.add_sized(vec2(width, super::row_height()), egui::Button::selectable(selected, text).truncate());
    crate::scenario::publish_target(&format!("dialog:list:{key}"), response.rect);
    response
}

/// A reusable draggable document-colour sample; caller supplies the existing domain payload.
/// Tokens: control_height/radius, border, accent, hover.
pub(crate) fn color_swatch<T: Send + Sync + 'static>(
    ui: &mut Ui,
    id: egui::Id,
    color: egui::Color32,
    selected: bool,
    tip: &str,
    payload: T,
) -> egui::Response {
    let t = crate::theme::Tokens::get();
    let response = ui
        .dnd_drag_source(id, payload, |ui| {
            let side = t.control_height + t.space_6;
            let (allocation, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
            let rect = allocation.shrink(t.space_2);
            if selected {
                ui.painter().rect_stroke(allocation, t.radius_lg, egui::Stroke::new(t.space_2, t.form_primary), egui::StrokeKind::Inside);
            }
            ui.painter().rect_filled(rect, t.control_radius, t.control_fill);
            if color.a() < u8::MAX {
                let tile = rect.width() / 6.0;
                for y in 0..6 {
                    for x in 0..6 {
                        if (x + y) % 2 == 0 {
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(rect.min + vec2(x as f32 * tile, y as f32 * tile), vec2(tile, tile)),
                                0.0,
                                t.border,
                            );
                        }
                    }
                }
            }
            ui.painter().rect(rect, t.control_radius, color, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
        })
        .response;
    let response = ui.interact(response.rect, id.with("click"), egui::Sense::click()).on_hover_text(tip);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    response
}

/// Wrapped samples with common token spacing. Keeps native drag-and-drop payloads intact.
pub(crate) fn swatch_grid<T>(ui: &mut Ui, values: &[T], mut each: impl FnMut(&mut Ui, &T)) {
    let t = Tokens::get();
    let columns = ((super::metrics::dimension(ui.available_width()) + t.space_2) / (t.control_height + t.space_8)).floor().clamp(1.0, 64.0) as usize;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(t.space_2, t.space_2);
        for row in values.chunks(columns.max(1)) {
            ui.horizontal(|ui| {
                for value in row {
                    each(ui, value);
                }
            });
        }
    });
}

/// A completion/suggestion entry; text and detail are application-supplied data.
pub(crate) struct PopupOption<'a> {
    pub text: &'a str,
    pub detail: &'a str,
    pub kind: &'a str,
    pub color: egui::Color32,
}

/// Focus-neutral suggestion popup. The owner handles navigation and completion semantics;
/// `selected` is the highlighted row, or `None` when no row is picked yet (#3663). Its rows
/// take no focus, and where they were drawn is kept under `id.with("rows")`, so an owner can
/// act on a press before the click lands (#3257).
/// Tokens: space_2, form_label_width, control_height, text_dim, scope control/font tokens.
pub(crate) fn popup_list(
    ctx: &egui::Context,
    id: egui::Id,
    anchor: egui::Rect,
    selected: Option<usize>,
    options: &[PopupOption<'_>],
    hint: &str,
) -> Option<usize> {
    let t = crate::theme::Tokens::get();
    let mut chosen = None;
    let mut rows = Vec::with_capacity(options.len());
    let popup = Container::new(ctx, Surface::Popup).area(id);
    let area = popup.order(egui::Order::Tooltip).fixed_pos(anchor.left_bottom() + egui::vec2(0.0, t.space_2)).show(ctx, |ui| {
        Container::popup_frame(ui.style())
            .fill(t.control_fill)
            .stroke(t.primary_border(t.dialog_border))
            .corner_radius(t.radius_lg)
            .inner_margin(t.space_4)
            .show(ui, |ui| {
                super::scope(ui, |ui| {
                    ui.set_width(anchor.width().max(t.form_card_width - t.space_16 * 2.0 - t.space_2));
                    for (index, option) in options.iter().enumerate() {
                        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height), egui::Sense::CLICK);
                        rows.push(rect);
                        let picked = selected == Some(index);
                        if picked || response.hovered() {
                            ui.painter().rect_filled(rect, t.control_radius, if picked { t.selected_bg } else { t.hover });
                        }
                        let inner = rect.shrink2(vec2(t.space_8, 0.0));
                        let kind = crate::i18n::tr_label(option.kind);
                        let kind_width = super::metrics::label_width(ui, kind);
                        let text_rect = egui::Rect::from_min_max(
                            inner.min,
                            egui::pos2((inner.right() - kind_width - t.space_8).max(inner.left()), inner.bottom()),
                        );
                        super::text::paint_text(
                            ui,
                            text_rect,
                            option.text,
                            egui::FontId::monospace(t.font_body),
                            if picked { t.form_primary } else { option.color },
                            false,
                        );
                        ui.painter().text(
                            inner.right_center(),
                            egui::Align2::RIGHT_CENTER,
                            kind,
                            egui::FontId::proportional(t.font_small),
                            t.form_dim,
                        );
                        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), picked, option.text));
                        let response = response.on_hover_text(option.detail);
                        crate::scenario::publish_handle(&format!("dialog:suggestion:{index}"), response.rect.center());
                        if response.clicked() {
                            chosen = Some(index);
                        }
                    }
                    super::divider(ui, "completion_detail");
                    if let Some(option) = selected.and_then(|index| options.get(index)) {
                        super::text(ui, option.detail, super::Text::Note("completion_detail"));
                    }
                    super::text(ui, hint, super::Text::Note("completion_hint"));
                })
            });
    });
    crate::scenario::publish_rect("dialog:suggestions", area.response.rect);
    ctx.data_mut(|d| d.insert_temp(id.with("rows"), rows));
    chosen
}

/// Compact draggable library thumbnail, retaining the caller's distinct payload type.
/// Tokens: space_16, radius_sm, border.
#[cfg(any(not(target_arch = "wasm32"), test))]
pub(crate) fn color_thumbnail<T: Send + Sync + 'static>(ui: &mut Ui, id: egui::Id, color: egui::Color32, tip: &str, payload: T) -> egui::Response {
    let t = Tokens::get();
    ui.dnd_drag_source(id, payload, |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(t.space_16, t.space_16), egui::Sense::hover());
        ui.painter().rect(rect, t.radius_sm, color, t.control_border_stroke(t.border), egui::StrokeKind::Inside);
    })
    .response
    .on_hover_text(tip)
}
