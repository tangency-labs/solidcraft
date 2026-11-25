//! Weld header and illustrated control compositions from the shared kit.
use super::*;

pub(super) fn type_choice(ui: &mut Ui, t: &Tokens, m: &mut Measurements, rect: Rect, info: &TypeInfo, on: bool) -> egui::Response {
    let response = at_button(ui, m, rect, info.icon, "", if on { Button::SmallSelected } else { Button::Normal });
    let icon = Rect::from_center_size(pos2(rect.center().x, rect.top() + t.space_8 + t.space_24 / 2.0), vec2(t.space_24, t.space_24));
    kit::icons::paint_icon(ui.painter(), icon, info.icon, 1.0);
    // The icon and the two text lines have separate slots: wrapping never raises text over the icon.
    let slot = Rect::from_min_max(pos2(rect.left() + t.space_4, icon.bottom() + t.space_2), rect.max - vec2(t.space_4, t.space_2));
    let font = if on { kit::type_face::semibold(t.font_body) } else { FontId::proportional(t.font_body) };
    let mut job = egui::text::LayoutJob::single_section(
        tr(info.label).into(),
        egui::TextFormat { font_id: font, color: t.form_ink, line_height: Some(t.space_14), ..Default::default() },
    );
    job.wrap.max_width = slot.width();
    job.wrap.max_rows = 2;
    job.halign = Align::Center;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    // Centre-aligned galleys are anchored on x=0, rather than their left edge.
    let at = pos2(slot.center().x, slot.top());
    #[cfg(test)]
    m.type_choices.push(ChoiceMeasurements { button: rect, icon, label: galley.rect.translate(at.to_vec2()), lines: galley.rows.len() });
    ui.painter().with_clip_rect(slot.intersect(ui.clip_rect())).galley(at, galley, t.form_ink);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, tr(info.label)));
    response.backing_on_hover_text(tr(info.label))
}

pub(super) fn dashed_outline(ui: &Ui, rect: Rect, t: &Tokens) {
    // add_choice is square and paints a plus. Reuse the kit's dash helper for this wide caption button.
    let stroke = t.control_border_stroke(t.control_border);
    let mut perimeter = Vec::new();
    egui::epaint::tessellator::path::rounded_rectangle(
        &mut perimeter,
        egui::epaint::RoundedRect::new(rect.shrink(stroke.width / 2.0), f32::from(t.control_radius)),
    );
    if let Some(first) = perimeter.first().copied() {
        perimeter.push(first);
    }
    ui.painter().extend(kit::dashed_line(&perimeter, ui.clip_rect(), stroke, t.space_3, t.space_3));
}

pub(super) fn more_button(ui: &mut Ui, m: &mut Measurements, rect: Rect, t: &Tokens) -> egui::Response {
    let mut control = child(ui, rect, "more");
    // The kit's styled Ghost still supplies an explicit stroke to egui. Its plain text action
    // has no frame override, so the shared dash helper is the only border painted here.
    let response = control.with_layout(Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| kit::text_button(ui, "", true)).inner;
    m.widgets.push(response.rect);
    dashed_outline(ui, rect, t);
    response
}

/// Height of the pinned symbol row above the shared Cancel / OK bar.
pub(super) fn symbol_band_height(t: &Tokens) -> f32 {
    t.control_height * 2.0 + t.space_16
}

/// The live symbol as a full-width button in its own pinned row; true when clicked.
pub(super) fn symbol_band(ui: &mut Ui, height: f32, state: &WeldPanelState, t: &Tokens, m: &mut Measurements) -> bool {
    let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    rule(ui, t, band.top());
    let rect = band.shrink2(vec2(t.space_12, t.space_8));
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return false;
    }
    let response = at_button(ui, m, rect, "symbol", "", Button::Normal).backing_on_hover_text(tr("ui.weld.symbol_tip"));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("ui.weld.symbol_tip")));
    symbol::paint(ui, rect, state, t);
    m.preview = Some(rect);
    m.define = Some(rect);
    solidcraft_ui_registry::publish_rect("weld:preview", rect);
    solidcraft_ui_registry::publish_target("weld:define_symbol", rect);
    response.clicked()
}
