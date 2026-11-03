//! Shared chord keycaps and the Preferences title-bar search field.
use crate::{icons, theme::Tokens};
use egui::{FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

fn chord_parts(chord: &str) -> Vec<&str> {
    // A literal plus key follows the separator, as in Ctrl++; keep its own full keycap.
    if let Some(prefix) = chord.strip_suffix('+') {
        let prefix = prefix.strip_suffix('+').unwrap_or(prefix);
        let mut parts: Vec<_> = prefix.split('+').filter(|part| !part.is_empty()).collect();
        parts.push("+");
        parts
    } else {
        chord.split('+').collect()
    }
}

/// Natural width of a complete chord in the same mono keycaps used by command search.
pub(crate) fn shortcut_binding_width(ui: &Ui, chord: &str) -> f32 {
    let t = Tokens::get();
    if chord.is_empty() {
        return super::ContentWidth::label(ui, crate::i18n::tr("prefs.shortcuts_not_assigned")) + t.space_8;
    }
    let parts = chord_parts(chord);
    parts
        .iter()
        .map(|part| ui.painter().layout_no_wrap((*part).to_owned(), FontId::monospace(t.font_small), t.form_dim).size().x + t.space_10)
        .sum::<f32>()
        + t.space_4 * parts.len().saturating_sub(1) as f32
}

/// One focusable binding, with a separate full keycap for every modifier and key.
/// Read-only callers keep exactly the same caps without an interactive recorder target.
pub(crate) fn shortcut_binding(ui: &mut Ui, chord: &str, editable: bool) -> Response {
    let t = Tokens::get();
    let (rect, response) =
        ui.allocate_exact_size(vec2(shortcut_binding_width(ui, chord), t.control_height), if editable { Sense::click() } else { Sense::hover() });
    if editable && response.clicked() {
        response.request_focus();
    }
    let mut x = rect.left();
    if chord.is_empty() {
        let text = crate::i18n::tr("prefs.shortcuts_not_assigned");
        let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(t.font_label), t.form_dim);
        ui.painter().galley(pos2(x + t.space_4, rect.center().y - galley.size().y / 2.0), galley, t.form_dim);
    } else {
        for part in chord_parts(chord) {
            let width = ui.painter().layout_no_wrap(part.to_owned(), FontId::monospace(t.font_small), t.form_dim).size().x + t.space_10;
            super::CommandSearch::key(ui, pos2(x + width, rect.center().y), part);
            x += width + t.space_4;
        }
    }
    if response.has_focus() || (editable && response.hovered()) {
        ui.painter().rect_stroke(rect, t.radius_sm, Stroke::new(1.0, t.form_primary), egui::StrokeKind::Outside);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            if editable { egui::WidgetType::Button } else { egui::WidgetType::Label },
            ui.is_enabled(),
            if chord.is_empty() { crate::i18n::tr("prefs.shortcuts_not_assigned") } else { chord },
        )
    });
    response
}

/// Native text editing with the same token frame and leading magnifier as command search.
pub(crate) fn preferences_search_field(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    search_with_icon(ui, id, value, hint)
}

fn search_with_icon(ui: &mut Ui, id: egui::Id, value: &mut String, hint: &str) -> Response {
    let t = Tokens::get();
    let height = t.form_close_size;
    let (r, frame) = ui.allocate_exact_size(vec2(super::metrics::dimension(ui.available_width()), height), Sense::click());
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(
        r,
        t.control_radius,
        t.control_fill,
        if focused { Stroke::new(1.0, t.form_primary) } else { t.control_border_stroke(t.control_border) },
        egui::StrokeKind::Inside,
    );
    let side = t.space_14;
    let icon = Rect::from_center_size(pos2(r.left() + t.space_8 + side * 0.5, r.center().y), vec2(side, side));
    icons::paint_ink(ui.painter(), icon, "zoom", t.form_dim);
    let show_clear = !value.is_empty();
    let clear_side = if show_clear { height } else { super::CommandSearch::key(ui, r.right_center() - vec2(t.space_6, 0.0), "Ctrl F") + t.space_12 };
    crate::scenario::publish_rect("prefs:search:frame", r);
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
