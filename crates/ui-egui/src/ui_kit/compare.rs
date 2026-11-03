//! Shared image-comparison divider and corner chips. Geometry and ink use kit tokens.
use crate::{scenario, theme::Tokens};
use egui::{Rect, Sense, Ui, pos2, vec2};

pub(crate) fn comparison_position(position: f32) -> f32 {
    if position.is_finite() { position.clamp(0.05, 0.95) } else { 0.5 }
}

/// A vertical wipe handle. The caller owns its command and suppresses canvas navigation while captured.
pub(crate) fn comparison_divider(ui: &mut Ui, canvas: Rect, position: &mut f32, label: &str) -> egui::Response {
    let t = Tokens::get();
    *position = comparison_position(*position);
    let x = canvas.left() + canvas.width() * *position;
    let rail = Rect::from_center_size(pos2(x, canvas.center().y), vec2(t.space_16, canvas.height()));
    let mut response = ui.interact(rail, ui.id().with("comparison_divider"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        *position = comparison_position((pointer.x - canvas.left()) / canvas.width().max(1.0));
        response.mark_changed();
    }
    comparison_divider_chrome(ui, canvas, *position);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Slider, ui.is_enabled(), label));
    scenario::publish_target("render:compare:divider", rail);
    response
}

pub(crate) fn comparison_divider_chrome(ui: &Ui, canvas: Rect, position: f32) {
    let t = Tokens::get();
    let x = canvas.left() + canvas.width() * comparison_position(position);
    let painter = ui.painter().with_clip_rect(canvas.intersect(ui.clip_rect()));
    let accent = crate::theme::TabAccent::current(ui.ctx());
    painter.vline(x, canvas.y_range(), egui::Stroke::new(t.space_2, accent.ink));
    let handle = Rect::from_center_size(pos2(x, canvas.center().y), vec2(t.space_16, t.space_24 + t.space_8));
    painter.rect_filled(handle, t.control_radius, t.panel);
    painter.rect_stroke(handle, t.control_radius, t.control_border_stroke(accent.ink), egui::StrokeKind::Inside);
    for offset in [-t.space_6, 0.0, t.space_6] {
        painter.circle_filled(handle.center() + vec2(0.0, offset), t.space_2, accent.ink);
    }
    scenario::publish_rect("render:compare:handle", handle);
}

/// Token-painted chip anchored inside either clipped half of the image canvas.
pub(crate) fn comparison_badge(ui: &mut Ui, bounds: Rect, label: &str, right: bool) {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(t.font_label), t.form_ink);
    let size = vec2((galley.size().x + t.space_16).min((bounds.width() - t.space_16).max(0.0)), t.control_height);
    let x = if right { bounds.right() - size.x - t.space_8 } else { bounds.left() + t.space_8 };
    let rect = Rect::from_min_size(pos2(x, bounds.top() + t.space_8), size);
    let painter = ui.painter().with_clip_rect(bounds.intersect(ui.clip_rect()));
    painter.rect_filled(rect, t.control_radius, t.panel);
    painter.rect_stroke(rect, t.control_radius, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    painter.with_clip_rect(rect).galley(pos2(rect.left() + t.space_8, rect.center().y - galley.size().y * 0.5), galley, t.form_ink);
    scenario::publish_rect(if right { "render:compare:right-label" } else { "render:compare:left-label" }, rect);
}
