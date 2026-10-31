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
    /// Explicit focus requested by the owning form when a setting is opened for editing.
    pub focus: Option<usize>,
    /// Per-option availability; omitted entries are enabled. Unavailable options stay faded.
    pub enabled: &'a [bool],
    /// Optional per-option refusal reasons. Disabled options remain visible and explain why.
    pub disabled: &'a [Option<&'a str>],
    /// Optional stable scenario targets, independent of translated option labels.
    pub targets: &'a [&'a str],
    /// Optional leading icons retain the same text-segment geometry and accessibility.
    pub icons: &'a [&'a str],
    pub workspace_accent: bool,
    pub dense: bool,
    pub small: bool,
    pub states: &'a [bool],
    /// Per-segment disabled explanation; unavailable choices remain visible.
    pub disabled_reasons: &'a [Option<&'a str>],
    pub compact: bool,
    pub primary: bool,
    pub intrinsic: bool,
    pub proportional: bool,
}

pub(crate) fn segmented(ui: &mut Ui, selected: &mut usize, options: &[&str], config: Segments<'_>) -> bool {
    crate::prefs_search::note_unlabelled(ui, options);
    if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, options) {
        return false;
    }
    if crate::prefs_search::querying(ui) && !crate::prefs_search::shows_all(ui) {
        crate::prefs_search::reveal(ui);
    }
    let Segments {
        focus,
        enabled,
        disabled,
        targets,
        icons,
        workspace_accent,
        dense,
        small,
        states,
        disabled_reasons,
        compact,
        primary,
        intrinsic,
        proportional,
    } = config;
    let accent = crate::theme::TabAccent::current(ui.ctx());
    let t = Tokens::get();
    let catalog = super::layout::in_catalog(ui);
    let font_size = if small || catalog { t.font_small } else { t.font_label };
    // Callers pass English source labels or text they translated; either way a segment shows it
    // translated, as the dropdown it falls back to does. Handles keep the option as passed.
    let shown: Vec<&str> = options.iter().map(|option| crate::i18n::tr_label(option)).collect();
    let natural: Vec<_> = shown
        .iter()
        .enumerate()
        .map(|(index, option)| {
            ui.painter().layout_no_wrap((*option).to_owned(), crate::fonts::semibold(font_size), t.form_ink).size().x.ceil()
                + t.space_16
                + if icons.get(index).is_some() { t.space_14 + t.space_6 + t.space_4 } else { 0.0 }
        })
        .collect();
    let total = natural.iter().sum::<f32>();
    let segment_width = dimension(ui.available_width()) / options.len().max(1) as f32;
    let widest = shown
        .iter()
        .map(|option| ui.painter().layout_no_wrap((*option).to_owned(), crate::fonts::semibold(font_size), t.form_ink).size().x)
        .fold(0.0, f32::max);
    // Judged on the whole row, which is what a scrollbar narrows, not on one segment.
    let need = if intrinsic { total } else { (widest + 2.0 * t.space_8) * options.len() as f32 };
    if enabled.is_empty()
        && disabled.is_empty()
        && disabled_reasons.is_empty()
        && !proportional
        && !compact
        && !super::fits_steadily(ui, label(ui), need, segment_width * options.len() as f32)
    {
        let before = *selected;
        let id = format!("segments_fallback_{}", label(ui));
        dropdown(ui, &id, selected, options);
        return *selected != before;
    }
    let desired = if intrinsic { total.min(dimension(ui.available_width())) } else { dimension(ui.available_width()) };
    let (rect, _) = ui.allocate_exact_size(
        vec2(
            desired,
            if catalog {
                t.space_16
            } else if small {
                t.control_height - t.space_4
            } else if dense {
                t.control_height - t.space_2
            } else {
                row_height()
            },
        ),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, t.control_radius, t.control_fill);
    let equal_width = rect.width() / options.len().max(1) as f32;
    let mut clicked = false;
    let mut x = rect.left();
    for (index, (option, text)) in options.iter().zip(&shown).enumerate() {
        let width =
            if (intrinsic || proportional) && total > 0.0 { natural.get(index).copied().unwrap_or(0.0) * rect.width() / total } else { equal_width };
        let segment = egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(width, rect.height()));
        x += width;
        let reason = disabled_reasons.get(index).copied().flatten().or_else(|| disabled.get(index).copied().flatten());
        let id = ui.make_persistent_id(("segment", index));
        // The row is already allocated: a child scopes disabled painting and interaction
        // without advancing the parent's cursor once per segment.
        let mut segment_ui = ui.new_child(egui::UiBuilder::new().max_rect(segment));
        if !enabled.get(index).copied().unwrap_or(true) || reason.is_some() {
            segment_ui.disable();
        }
        let ui = &mut segment_ui;
        let segment_enabled = ui.is_enabled();
        let response = ui.interact(segment, id, egui::Sense::click());
        if focus == Some(index) && segment_enabled && ui.is_visible() && ui.is_rect_visible(segment) {
            response.request_focus();
        }
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
                } else if on && workspace_accent {
                    accent.soft
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
        let font = if on { crate::fonts::semibold(font_size) } else { egui::FontId::proportional(font_size) };
        let galley = ui.painter().layout_no_wrap((*text).to_string(), font.clone(), t.form_ink);
        let icon_width = if icons.get(index).is_some() { t.space_14 + t.space_6 } else { 0.0 };
        let text_rect = egui::Rect::from_min_size(
            egui::pos2(segment.center().x - (galley.size().x + icon_width) / 2.0 + icon_width, segment.top()),
            vec2(galley.size().x, segment.height()),
        );
        if let Some(icon) = icons.get(index) {
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(text_rect.left() - t.space_6 - t.space_14 / 2.0, segment.center().y),
                vec2(t.space_14, t.space_14),
            );
            crate::icons::paint_ink(ui.painter(), icon_rect, icon, if on && workspace_accent { accent.ink } else { t.form_ink });
        }
        let text_rect = if compact { text_rect.intersect(segment.shrink2(vec2(t.space_4, 0.0))) } else { text_rect };
        paint_text(
            ui,
            text_rect,
            text,
            font,
            if !segment_enabled {
                t.form_dim
            } else if on && primary {
                t.on_accent
            } else if on && workspace_accent {
                accent.ink
            } else if on {
                t.selected_ink
            } else {
                t.form_ink
            },
            false,
        );
        t.button_bevel_with_radius(ui, &response, on, rounding);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, segment_enabled, on, *text));
        scenario::publish_target(&format!("dialog:{}:{option}", label(ui)), segment);
        if let Some(target) = targets.get(index) {
            let visible = segment.intersect(ui.clip_rect());
            if visible.is_positive() {
                scenario::publish_target(target, visible);
            }
        }
        let response = if let Some(reason) = reason { response.on_disabled_hover_text(reason) } else { response.on_hover_text(*text) };
        if response.clicked() && segment_enabled {
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
    let catalog = super::layout::in_catalog(ui);
    let accent = crate::theme::TabAccent::current(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(vec2(dimension(ui.available_width()), if catalog { t.space_24 } else { row_height() }), egui::Sense::hover());
    if !catalog {
        ui.painter().rect_filled(rect, t.control_radius, t.control_fill);
    }
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
            ui.painter().rect_filled(
                segment,
                rounding,
                if on && catalog {
                    accent.soft
                } else if on {
                    t.selected_bg
                } else {
                    t.hover
                },
            );
        }
        if index > 0 && !catalog {
            ui.painter().vline(segment.left(), segment.y_range(), t.control_border_stroke(t.control_border));
        }
        // A square icon area inside the segment: dash gaps and dots need the room to read apart.
        let side = (segment.width().min(segment.height()) - t.space_4).max(t.space_12);
        let icon_rect = egui::Rect::from_center_size(segment.center(), vec2(side, side));
        if catalog {
            let icon_rect = egui::Rect::from_center_size(segment.center(), vec2(t.space_14, t.space_14));
            crate::icons::paint_ink(ui.painter(), icon_rect, option.icon, if on { accent.ink } else { t.form_dim });
        } else {
            crate::icons::paint_icon(ui.painter(), icon_rect, option.icon, f32::from(t.icon.a()) / 255.0);
        }
        t.button_bevel_with_radius(ui, &response, on, rounding);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, option.key));
        scenario::publish_handle(&format!("dialog:{}:{}", label(ui), option.key), segment.center());
        scenario::publish_target(option.key, segment);
        let response = response.on_hover_text(&option.tip);
        if response.clicked() {
            *selected = index;
            clicked = true;
        }
    }
    if !catalog && t.borders.bevel.is_none() {
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

/// An on/off icon button inside a rectangle the caller placed, so rows keep their columns: the
/// same state, hit target and accessible name as [`toggle_icon`].
/// Tokens: `space_3, icon/fill, selected_bg, hover; inherited control surface tokens`.
pub(crate) fn toggle_icon_at(ui: &mut Ui, rect: egui::Rect, row: &str, icon: &str, label: &str, selected: bool) -> egui::Response {
    let t = Tokens::get();
    let response = ui.interact(rect, ui.make_persistent_id(("toggle_icon", icon, label)), egui::Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    // The state is drawn under the icon: a filled field while on, a hover wash while pointed at.
    if selected {
        ui.painter().rect_filled(rect, t.control_radius, t.selected_bg);
        ui.painter().rect_stroke(rect, t.control_radius, egui::Stroke::new(1.0, t.accent), egui::StrokeKind::Inside);
    } else if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    crate::icons::paint_icon(ui.painter(), rect.shrink(t.space_3), icon, f32::from(t.icon.a()) / 255.0);
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, crate::i18n::tr_label(label)));
    crate::scenario::publish_handle(&format!("dialog:{row}:{label}"), rect.center());
    crate::scenario::publish_rect(&format!("dialog:{row}:{label}"), rect);
    response.on_hover_text(crate::i18n::tr_label(label))
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
    if super::layout::in_catalog(ui) {
        let accent = crate::theme::TabAccent::current(ui.ctx());
        let width = ui.painter().layout_no_wrap(text.to_string(), crate::fonts::semibold(t.font_small), t.form_ink).size().x + t.space_16;
        let (rect, response) = ui.allocate_exact_size(vec2(width, t.space_16 + t.space_4), egui::Sense::click());
        ui.painter().rect_filled(rect, t.radius_xl, if selected { accent.soft } else { t.control_fill });
        paint_text(
            ui,
            rect.shrink2(vec2(t.space_8, 0.0)),
            text,
            if selected { crate::fonts::semibold(t.font_small) } else { egui::FontId::proportional(t.font_small) },
            if selected { accent.ink } else { t.form_ink },
            false,
        );
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, text));
        crate::scenario::publish_target(&format!("dialog:list:{key}"), rect);
        return response;
    }
    let width = (ui.painter().layout_no_wrap(text.to_owned(), crate::fonts::semibold(t.font_label), t.text).size().x + t.space_24)
        .clamp(t.control_height * 2.0, t.form_card_width);
    let response = ui.add_sized(vec2(width, super::row_height()), egui::Button::selectable(selected, text).truncate());
    crate::scenario::publish_target(&format!("dialog:list:{key}"), response.rect);
    response
}

/// Thumbnail variant of the shared selection chip. No document or render-job ownership.
#[derive(Default)]
pub(crate) struct ThumbnailChoice {
    pub texture: Option<egui::TextureId>,
    pub selected: bool,
    pub modified: bool,
    pub progress: Option<f32>,
}

pub(crate) fn thumbnail_choice(ui: &mut Ui, key: &str, text: &str, state: ThumbnailChoice) -> egui::Response {
    let t = Tokens::get();
    let thumb_size = vec2(t.control_height + t.space_24, t.control_height + t.space_2);
    let height = thumb_size.y + t.space_8;
    let font = crate::fonts::semibold(t.font_small);
    let width = ui.painter().layout_no_wrap(text.to_owned(), font.clone(), t.form_ink).size().x.min(t.form_label_width * 2.0)
        + thumb_size.x
        + t.space_8
        + t.space_14;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
    let accent = crate::theme::TabAccent::current(ui.ctx());
    let ink = if state.selected { accent.ink } else { t.form_ink };
    ui.painter().rect(
        rect,
        t.control_radius,
        if state.selected {
            accent.soft
        } else if response.hovered() {
            t.hover
        } else {
            t.control_fill
        },
        egui::Stroke::new(if state.selected { 1.5 } else { 1.0 }, if state.selected { accent.ink } else { t.control_border }),
        egui::StrokeKind::Inside,
    );
    let thumb = egui::Rect::from_min_size(rect.min + vec2(t.space_4, t.space_4), thumb_size);
    ui.painter().rect_filled(thumb, t.radius_sm, t.dialog_footer);
    if let Some(texture) = state.texture {
        egui::Image::new((texture, thumb.size())).corner_radius(t.radius_sm).paint_at(ui, thumb);
    } else {
        // Deterministic placeholder while loading, or when a worker is unavailable.
        ui.painter().hline(thumb.shrink(t.space_8).x_range(), thumb.center().y, t.control_border_stroke(t.control_border));
    }
    super::text::paint_text(
        ui,
        egui::Rect::from_min_max(egui::pos2(thumb.right() + t.space_8, rect.top()), rect.max - vec2(t.space_10, 0.0)),
        text,
        font,
        ink,
        false,
    );
    if state.modified {
        let dot = egui::pos2(rect.right() - t.space_6, rect.top() + t.space_6);
        ui.painter().circle_filled(dot, t.space_3, t.warning);
        crate::scenario::publish_rect(&format!("dialog:list:{key}:modified"), egui::Rect::from_center_size(dot, vec2(t.space_6, t.space_6)));
    }
    if let Some(progress) = state.progress {
        let center = thumb.center();
        let radius = t.space_8;
        ui.painter().circle_filled(center, radius + t.space_3, t.dialog_body);
        ui.painter().circle_stroke(center, radius, egui::Stroke::new(t.space_2, t.control_border));
        let fraction = if progress.is_finite() { progress.clamp(0.0, 1.0) } else { 0.0 };
        let points: Vec<_> = (0..=32)
            .map(|i| {
                let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * fraction.max(0.03) * i as f32 / 32.0;
                center + radius * vec2(angle.cos(), angle.sin())
            })
            .collect();
        ui.painter().add(egui::Shape::line(points, egui::Stroke::new(t.space_2, accent.ink)));
        crate::scenario::publish_rect(&format!("dialog:list:{key}:progress"), thumb);
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), state.selected, text));
    crate::scenario::publish_target(&format!("dialog:list:{key}"), response.rect);
    response.on_hover_text(text)
}

/// Dashed add variant for thumbnail strips, sharing control tokens and button semantics.
pub(crate) fn add_choice(ui: &mut Ui, label: &str) -> egui::Response {
    let t = Tokens::get();
    let side = t.control_height + t.space_8;
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, t.control_radius, t.hover);
    }
    let r = rect.shrink(t.space_2);
    let mut outline = super::preferences::rounded_perimeter(r, f32::from(t.control_radius));
    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }
    ui.painter().add(egui::Shape::dashed_line(&outline, egui::Stroke::new(1.0, t.control_border), t.space_3, t.space_3));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(t.font_subtitle), t.form_dim);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    response.on_hover_text(label)
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

/// Rows a completion list shows before it scrolls.
const POPUP_LIST_ROWS: usize = 8;

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
    // Wide enough for the longest suggestion; still capped so a hostile name cannot fill the screen (#8533).
    let mono = egui::FontId::monospace(t.font_body);
    let kind_font = egui::FontId::proportional(t.font_small);
    let content_w = options
        .iter()
        .map(|option| {
            let text_w = ctx.fonts_mut(|fonts| fonts.layout_no_wrap(option.text.to_owned(), mono.clone(), option.color)).size().x;
            let kind = crate::i18n::tr_label(option.kind);
            let kind_w = ctx.fonts_mut(|fonts| fonts.layout_no_wrap(kind.to_owned(), kind_font.clone(), t.form_dim)).size().x;
            text_w + kind_w + t.space_8 * 3.0
        })
        .fold(0.0_f32, f32::max);
    let width = anchor.width().max(t.form_card_width - t.space_16 * 2.0 - t.space_2).max(content_w).min(t.form_card_width * 2.5);
    let popup = Container::new(ctx, Surface::Popup).area(id);
    let area = popup.order(egui::Order::Tooltip).fixed_pos(anchor.left_bottom() + egui::vec2(0.0, t.space_2)).show(ctx, |ui| {
        Container::popup_frame(ui.style())
            .fill(t.control_fill)
            .stroke(t.primary_border(t.dialog_border))
            .corner_radius(t.radius_lg)
            .inner_margin(t.space_4)
            .show(ui, |ui| {
                super::scope(ui, |ui| {
                    ui.set_width(width);
                    // A long list (every parameter, #9246) scrolls past its first rows and keeps
                    // the chosen row in view.
                    let scrolls = options.len() > POPUP_LIST_ROWS;
                    let followed = id.with("followed");
                    let follow = scrolls && selected.is_some() && ctx.data(|d| d.get_temp::<Option<usize>>(followed)) != Some(selected);
                    ctx.data_mut(|d| d.insert_temp(followed, selected));
                    let scroll = egui::ScrollArea::vertical()
                        .id_salt(id.with("scroll"))
                        .max_height(t.control_height * POPUP_LIST_ROWS as f32)
                        .auto_shrink([false, true])
                        .vscroll(scrolls);
                    scroll.show(ui, |ui| {
                        for (index, option) in options.iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height), egui::Sense::CLICK);
                            if follow && selected == Some(index) {
                                ui.scroll_to_rect_animation(rect, None, egui::style::ScrollAnimation::none());
                            }
                            // A row scrolled out of view is not under the pointer.
                            rows.push(if ui.is_rect_visible(rect) { rect.intersect(ui.clip_rect()) } else { egui::Rect::NOTHING });
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
                                mono.clone(),
                                if picked { t.form_primary } else { option.color },
                                false,
                            );
                            ui.painter().text(inner.right_center(), egui::Align2::RIGHT_CENTER, kind, kind_font.clone(), t.form_dim);
                            response
                                .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), picked, option.text));
                            // Tip the full name first: long identifiers still clip inside a capped popup.
                            let tip = if option.detail.is_empty() { option.text.to_owned() } else { format!("{}\n{}", option.text, option.detail) };
                            let response = response.on_hover_text(tip);
                            crate::scenario::publish_handle(&format!("dialog:suggestion:{index}"), response.rect.center());
                            if response.clicked() {
                                chosen = Some(index);
                            }
                        }
                    });
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
