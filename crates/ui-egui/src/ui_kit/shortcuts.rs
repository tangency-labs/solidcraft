//! Search-and-actions toolbar shared by settings tables and command lists.
use crate::{icons, theme::Tokens};
use egui::{Align, FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

pub(crate) struct SearchAction<'a> {
    pub label: &'a str,
    pub icon: Option<&'a str>,
    pub visible: bool,
    pub tooltip: Option<&'a str>,
}

/// One expanding search input followed by measured, untruncated action buttons.
/// Narrow hosts scroll the row horizontally rather than wrapping actions below its filter.
pub(crate) fn search_actions_row(
    ui: &mut Ui,
    id: egui::Id,
    value: &mut String,
    hint: &str,
    actions: &[SearchAction<'_>],
) -> (Response, Option<usize>) {
    let t = Tokens::get();
    let action_width: f32 = actions
        .iter()
        .filter(|a| a.visible)
        .map(|a| {
            let text = super::ContentWidth::label(ui, a.label);
            let padding = if a.icon.is_some() { t.space_10 * 2.0 + t.space_14 + t.space_6 } else { ui.spacing().button_padding.x * 2.0 };
            text + padding + t.space_8
        })
        .sum();
    let width = super::metrics::dimension(ui.available_width());
    let minimum = t.form_label_width * 2.0 + action_width;
    let shown = egui::ScrollArea::horizontal().id_salt(id.with("actions_scroll")).auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            let filter_width = (width.max(minimum) - action_width).max(0.0);
            let response = ui
                .allocate_ui_with_layout(vec2(filter_width, t.control_height), egui::Layout::left_to_right(Align::Center), |ui| {
                    search_with_icon(ui, id, value, hint, false)
                })
                .inner;
            let mut action = None;
            for (index, item) in actions.iter().enumerate().filter(|(_, a)| a.visible) {
                let response = if let Some(icon) = item.icon {
                    super::icon_label_button(ui, item.label, icon, true, false)
                } else {
                    super::button(ui, item.label, super::Button::Normal)
                };
                let response = if let Some(tooltip) = item.tooltip { response.on_hover_text(tooltip) } else { response };
                if response.clicked() {
                    action = Some(index);
                }
            }
            (response, action)
        })
        .inner
    });
    shown.inner
}

/// Native text editing with the same token frame and leading magnifier as command search.
pub(crate) fn preferences_search_field(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    search_with_icon(ui, id, value, hint, true)
}

fn search_with_icon(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str, titlebar: bool) -> Response {
    let t = Tokens::get();
    let height = if titlebar { t.form_close_size } else { t.control_height };
    let (r, frame) = ui.allocate_exact_size(vec2(super::metrics::dimension(ui.available_width()), height), Sense::click());
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(
        r,
        t.control_radius,
        t.control_fill,
        if focused { Stroke::new(1.0, t.form_primary) } else { t.control_border_stroke(t.control_border) },
        egui::StrokeKind::Inside,
    );
    let side = if titlebar { t.space_14 } else { t.space_16 };
    let icon = Rect::from_center_size(pos2(r.left() + t.space_8 + side * 0.5, r.center().y), vec2(side, side));
    icons::paint_ink(ui.painter(), icon, "zoom", t.form_dim);
    let show_clear = !value.is_empty();
    let clear_side = if show_clear {
        height
    } else if titlebar {
        super::CommandSearch::key(ui, r.right_center() - vec2(t.space_6, 0.0), "Ctrl F") + t.space_12
    } else {
        0.0
    };
    if titlebar {
        crate::scenario::publish_rect("prefs:search:frame", r);
    }
    let editor =
        Rect::from_min_max(pos2((icon.right() + t.space_6).min(r.right()), r.top()), pos2((r.right() - clear_side).max(icon.right()), r.bottom()));
    let response = ui
        .scope_builder(egui::UiBuilder::new().max_rect(editor), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(editor));
            let mut response = ui.add_sized(
                editor.size(),
                super::fields::text_input(egui::TextEdit::singleline(value))
                    .id(id)
                    .hint_text(crate::i18n::tr_label(hint))
                    .font(FontId::proportional(t.font_label))
                    .text_color(t.form_ink)
                    .margin(vec2(t.space_4, 0.0))
                    .clip_text(true)
                    .frame(egui::Frame::NONE),
            );
            super::text_menu::menu(ui, &mut response, value, super::text_menu::Field::LINE);
            response
        })
        .inner;
    let mut cleared = false;
    if show_clear {
        let clear_rect = Rect::from_center_size(pos2(r.right() - height * 0.5, r.center().y), vec2(height, height));
        let clear = ui.push_id(id.with("clear"), |ui| super::quiet_icon_button_at(ui, clear_rect, "close", "Clear")).inner;
        if id == egui::Id::new("prefs_search") {
            crate::scenario::publish_handle("prefs:search:clear", clear_rect.center());
        }
        if clear.clicked() {
            value.clear();
            cleared = true;
        }
    }
    if frame.clicked() && !cleared {
        response.request_focus();
    }
    super::layout::publish(ui, &response, egui::WidgetType::TextEdit);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), crate::i18n::tr_label(hint));
        info.current_text_value = Some(value.clone());
        info
    });
    response
}
