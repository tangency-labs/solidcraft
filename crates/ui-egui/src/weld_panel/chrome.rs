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

pub(super) fn header(ui: &mut Ui, state: &mut WeldPanelState, t: &Tokens, m: &mut Measurements, event: &mut WeldPanelEvent) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.form_header_height + t.space_4), Sense::hover());
    let icon = Rect::from_center_size(pos2(rect.left() + t.space_12 + t.space_12, rect.center().y), vec2(t.space_24, t.space_24));
    kit::icons::paint_icon(ui.painter(), icon, "weld", 1.0);
    ui.painter().text(
        pos2(icon.right() + t.space_8, rect.center().y),
        Align2::LEFT_CENTER,
        tr("ui.weld.title"),
        kit::type_face::semibold(t.font_heading),
        t.form_ink,
    );
    let width = ui.painter().layout_no_wrap(state.number.clone(), FontId::monospace(t.font_body), t.form_ink).size().x + t.space_16;
    let mut chip =
        child(ui, Rect::from_min_size(pos2(rect.right() - t.space_12 - width, rect.center().y - t.space_12), vec2(width, t.space_24)), "number");
    kit::settings_chip(&mut chip, &state.number, kit::SettingsChip { monospace: true, ..Default::default() })
        .backing_on_hover_text(tr("ui.weld.number_tip"));
    rule(ui, t, rect.bottom());
    m.sections.push(("header", rect));

    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height + 2.0 * t.space_16), Sense::hover());
    ui.painter().rect_filled(rect, 0, t.dialog_footer);
    let cancel_text = ui.painter().layout_no_wrap(tr("ui.weld.cancel").into(), FontId::proportional(t.font_label), t.form_ink);
    let done_text = ui.painter().layout_no_wrap(tr("ui.weld.done").into(), kit::type_face::semibold(t.font_label), t.on_accent);
    let hint_width = t.space_24 * 4.0;
    let gap = t.space_6;
    let minimum_preview = t.form_label_width + t.space_24 + t.space_16;
    let preferred_preview = t.form_label_width * 2.0 + t.space_10;
    let inner_width = rect.width() - 2.0 * t.space_12;
    let cancel_content = cancel_text.size().x.ceil();
    let done_content = done_text.size().x.ceil() + t.space_14 + t.space_6;
    // Keep all four items in the card: only the actions' horizontal padding compresses.
    let action_room = inner_width - hint_width - minimum_preview - 3.0 * gap;
    let action_padding = ((action_room - cancel_content - done_content) / 2.0).clamp(0.0, 2.0 * t.space_8);
    let cancel_width = cancel_content + action_padding;
    let done_width = done_content + action_padding;
    let preview_width = (inner_width - cancel_width - done_width - hint_width - 3.0 * gap).clamp(minimum_preview, preferred_preview);
    let symbol = Rect::from_min_size(rect.min + vec2(t.space_12, t.space_8), vec2(preview_width, t.form_header_height + t.space_4));
    let preview = at_button(ui, m, symbol, "symbol", "", Button::Normal).backing_on_hover_text(tr("ui.weld.symbol_tip"));
    preview.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("ui.weld.symbol_tip")));
    let symbol_defined = symbol::paint(ui, symbol, state, t);
    if preview.clicked() {
        *event = WeldPanelEvent::DefineSymbol;
    }
    let done = Rect::from_center_size(pos2(rect.right() - t.space_12 - done_width / 2.0, rect.center().y), vec2(done_width, t.control_height));
    let cancel = Rect::from_center_size(pos2(done.left() - gap - cancel_width / 2.0, rect.center().y), vec2(cancel_width, t.control_height));
    let hint = Rect::from_min_size(pos2(cancel.left() - gap - hint_width, symbol.top()), vec2(hint_width, symbol.height()));
    let mut job = egui::text::LayoutJob::single_section(
        tr(if symbol_defined { "ui.weld.symbol" } else { "ui.weld.symbol_undefined" }).into(),
        egui::TextFormat {
            font_id: FontId::proportional(t.font_caption),
            color: t.form_dim,
            line_height: Some(t.font_caption * 1.3),
            ..Default::default()
        },
    );
    job.wrap.max_width = hint.width();
    job.wrap.max_rows = 2;
    let hint_text = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let hint_at = pos2(hint.left(), hint.center().y - hint_text.size().y / 2.0);
    #[cfg(test)]
    {
        m.strip_hint = Some(hint);
        m.strip_hint_text = Some(hint_text.rect.translate(hint_at.to_vec2()));
        m.strip_hint_lines = hint_text.rows.len();
    }
    ui.painter().with_clip_rect(hint.intersect(ui.clip_rect())).galley(hint_at, hint_text, t.form_dim);
    let cancel_response = at_button(ui, m, cancel, "cancel", "", Button::Normal);
    cancel_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("ui.weld.cancel")));
    ui.painter().galley(cancel.center() - cancel_text.size() / 2.0, cancel_text, t.form_ink);
    if cancel_response.clicked() {
        *event = WeldPanelEvent::Cancel;
    }
    let done_response = at_button(ui, m, done, "done", "", Button::Primary);
    done_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tr("ui.weld.done")));
    let check_left = done.center().x - (t.space_14 + t.space_6 + done_text.size().x) / 2.0;
    let check = Rect::from_center_size(pos2(check_left + t.space_14 / 2.0, done.center().y), vec2(t.space_14, t.space_14));
    #[cfg(test)]
    {
        m.done_check = Some(check);
    }
    let point = |x: f32, y: f32| check.min + vec2(x, y) * (t.space_14 / t.space_24);
    ui.painter().add(egui::Shape::line(vec![point(5.0, 12.5), point(9.5, 17.0), point(19.0, 7.0)], egui::Stroke::new(t.space_2, t.on_accent)));
    ui.painter().galley(pos2(check.right() + t.space_6, done.center().y - done_text.size().y / 2.0), done_text, t.on_accent);
    if done_response.clicked() {
        *event = WeldPanelEvent::Done;
    }
    rule(ui, t, rect.bottom());
    m.sections.push(("strip", rect));
}
