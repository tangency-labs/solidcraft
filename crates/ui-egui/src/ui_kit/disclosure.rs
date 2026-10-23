//! One disclosure drawing for sections, trees, cards and window folds.
use crate::theme::Tokens;
use egui::{Color32, Painter, Pos2, Rect, Response, Ui, vec2};

/// Content visible: down. Content hidden: right. No animation, matching kit sections.
/// Tokens: form_section_icon_size; the caller supplies a semantic ink token.
pub(crate) fn disclosure(painter: &Painter, center: Pos2, open: bool, ink: Color32) {
    let size = Tokens::get().form_section_icon_size;
    let rect = Rect::from_center_size(center, vec2(size, size));
    crate::icons::paint_ink(painter, rect, if open { "chevron" } else { "chevron_right" }, ink);
}

/// Fold action in existing chrome. Stable identity across state changes, with the caller's
/// translated tooltip and scenario handles. Tokens: control_radius, hover, form_ink/dim.
pub(crate) fn disclosure_button_at(ui: &mut Ui, rect: Rect, open: bool, label: &str) -> Response {
    disclosure_button_with_hover(ui, rect, open, label, false)
}

/// Panel fold action with the browser's shared blue hover tint.
pub(crate) fn panel_disclosure_button_at(ui: &mut Ui, rect: Rect, open: bool, label: &str) -> Response {
    disclosure_button_with_hover(ui, rect, open, label, true)
}

fn disclosure_button_with_hover(ui: &mut Ui, rect: Rect, open: bool, label: &str, panel: bool) -> Response {
    let t = Tokens::get();
    let response = ui.interact(rect, ui.make_persistent_id("disclosure_button"), egui::Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    if response.hovered() {
        if panel {
            super::panel_header_hover(ui.painter(), rect);
        } else {
            ui.painter().rect_filled(rect, t.control_radius, t.hover);
        }
    }
    disclosure(ui.painter(), rect.center(), open, if response.hovered() || response.has_focus() { t.form_ink } else { t.form_dim });
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::CollapsingHeader, ui.is_enabled(), open, label));
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(label)
}

/// Dropdowns always point down, independently of popup state. Preserve their existing size.
pub(crate) fn dropdown_indicator(painter: &Painter, rect: Rect, ink: Color32) {
    crate::icons::paint_ink(painter, rect, "chevron", ink);
}

#[cfg(test)]
#[path = "disclosure_tests.rs"]
mod tests;
