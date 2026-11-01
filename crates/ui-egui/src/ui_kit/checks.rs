//! The only checkbox box and check glyph, shared by inline, palette, grid and menu rows.
use crate::theme::Tokens;
use egui::{Rect, Response, Ui, pos2, vec2};

/// Fixed logical size and pixel-aligned origin, independent of a caller's fonts or spacing.
pub(super) fn box_at(ui: &Ui, center: egui::Pos2) -> Rect {
    let t = Tokens::get();
    let ppp = ui.ctx().pixels_per_point();
    let min = center - vec2(t.space_14, t.space_14) / 2.0;
    Rect::from_min_size(pos2((min.x * ppp).round() / ppp, (min.y * ppp).round() / ppp), vec2(t.space_14, t.space_14))
}

/// One path for every tick, including menu rows. Mixed state uses a horizontal bar.
pub(super) fn paint_check_glyph(painter: &egui::Painter, square: Rect, mixed: bool, ink: egui::Color32) {
    let t = Tokens::get();
    let mark = square.shrink(t.space_3);
    let points = if mixed { vec![mark.left_center(), mark.right_center()] } else { vec![mark.left_center(), mark.center_bottom(), mark.right_top()] };
    painter.add(egui::Shape::line(points, egui::Stroke::new(t.space_2, ink)));
}

/// A small action that accepts what is on the canvas, shown by the tick alone.
/// Tokens: `control_height, space_3/14/24/4, form_primary; inherited control surface tokens`.
pub(crate) fn check_button(ui: &mut Ui, target: &str, label: &str) -> Response {
    let t = Tokens::get();
    let response = ui.add(egui::Button::new("").min_size(vec2(t.space_24 + t.space_4, t.control_height)));
    paint_check_glyph(ui.painter(), Rect::from_center_size(response.rect.center(), vec2(t.space_14, t.space_14)), false, t.form_primary);
    t.button_bevel(ui, &response, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    crate::scenario::publish_handle(target, response.rect.center());
    crate::scenario::publish_rect(target, response.rect);
    response.on_hover_text(label)
}

/// All states use the same geometry. Disabled opacity is inherited from egui's disabled UI.
pub(super) fn paint_box(ui: &Ui, square: Rect, response: &Response, on: bool, mixed: bool) {
    let t = Tokens::get();
    let selected = on || mixed;
    let border = if selected || response.hovered() || response.has_focus() { t.form_primary } else { t.control_border };
    let fill = if t.borders.bevel.is_some() {
        t.input_fill()
    } else if selected {
        t.form_primary_fill
    } else if response.hovered() {
        t.hover
    } else {
        t.control_fill
    };
    ui.painter().rect(
        square,
        t.radius_sm,
        fill,
        if selected || response.hovered() || response.has_focus() { egui::Stroke::new(1.0, border) } else { t.control_border_stroke(border) },
        egui::StrokeKind::Inside,
    );
    if let Some(bevel) = t.borders.bevel {
        bevel.paint(ui.painter(), square, t.radius_sm, true, response.enabled());
    }
    if selected {
        paint_check_glyph(ui.painter(), square, mixed, if t.borders.bevel.is_some() { t.form_ink } else { t.on_accent });
    }
    if response.has_focus() {
        ui.painter().rect_stroke(square.expand(t.space_2), t.radius_sm, egui::Stroke::new(1.0, t.form_primary), egui::StrokeKind::Outside);
    }
}

/// Checkboxes only via ui_kit::checkbox. Labels, accessibility, keyboard and response stay native.
pub(crate) fn checkbox(ui: &mut Ui, label: &str, value: &mut bool) -> Response {
    checkbox_indeterminate(ui, label, value, false)
}

/// Mixed values become checked when activated; the caller owns whether selection is mixed.
/// Tokens: space_14/3/2/6, radius_sm, control_fill/border, form_primary/ink, hover, on_accent.
pub(crate) fn checkbox_indeterminate(ui: &mut Ui, label: &str, value: &mut bool, mixed: bool) -> Response {
    crate::prefs_search::note_unlabelled(ui, &[label]);
    if crate::prefs_search::querying(ui) && !crate::prefs_search::visible(ui, &[label]) {
        return crate::prefs_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_label);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), t.form_ink);
    let gap = if label.is_empty() { 0.0 } else { t.space_6 };
    let width = (t.space_14 + gap + galley.size().x).max(ui.spacing().interact_size.y).min(super::metrics::dimension(ui.available_width()));
    let height = ui.spacing().interact_size.y.max(t.space_14).max(galley.size().y);
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, height), egui::Sense::click());
    crate::prefs_search::remember(ui, label, rect);
    crate::prefs_search::follow(ui, label, rect);
    if response.clicked() {
        *value = mixed || !*value;
        response.mark_changed();
    }
    let square = box_at(ui, pos2(rect.left() + t.space_14 / 2.0, rect.center().y));
    paint_box(ui, square, &response, *value, mixed && !response.changed());
    let text_rect = Rect::from_min_max(pos2((square.right() + gap).min(rect.right()), rect.top()), rect.max);
    super::text::paint_text(ui, text_rect, label, font, t.form_ink, false);
    response.widget_info(|| {
        let accessible = if label.is_empty() { crate::i18n::tr_label(super::row_label(ui)) } else { label };
        let mut info = egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, accessible);
        if mixed && !response.changed() {
            info.selected = None;
        }
        info
    });
    super::layout::publish_geometry(ui, &response);
    response
}

#[cfg(test)]
#[path = "checks_tests.rs"]
pub(super) mod tests;
