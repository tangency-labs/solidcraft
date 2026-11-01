//! Compact navigation preference compositions and explanatory figures.
use super::{metrics::dimension, text::paint_text};
use crate::theme::Tokens;
use egui::{FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

/// One mouse silhouette beside the actual mapping selector and binding legend.
/// The figure is descriptive content, not an icon asset or an application viewport.
pub(crate) fn settings_mouse_mapping(ui: &mut Ui, bindings: &[(Vec<String>, String)], mapping: impl FnOnce(&mut Ui)) {
    let keys: Vec<Vec<&str>> = bindings.iter().map(|(keys, _)| keys.iter().map(String::as_str).collect()).collect();
    let rows: Vec<(&[&str], &str)> = bindings.iter().zip(&keys).map(|((_, action), keys)| (keys.as_slice(), action.as_str())).collect();
    if crate::prefs_search::suppress_chrome(ui) {
        mapping(ui);
        return;
    }
    let t = Tokens::get();
    let available = dimension(ui.available_width());
    let figure_size = vec2(t.space_24 * 4.0, t.space_24 * 5.0);
    let gap = t.space_24 - t.space_2;
    let content_width = if available >= figure_size.x + gap + t.space_24 * 12.0 { available - figure_size.x - gap } else { available };
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let (figure, _) = ui.allocate_exact_size(vec2(figure_size.x.min(available), figure_size.y), Sense::hover());
        super::preferences_figure_data::paint_mouse_mapping(&ui.painter_at(figure), figure);
        ui.allocate_ui_with_layout(vec2(content_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            mapping(ui);
            ui.add_space(t.space_6);
            super::settings_bindings(ui, &rows);
        });
    });
}

fn axis_columns(rect: Rect) -> [Rect; 5] {
    let t = Tokens::get();
    let gap = t.space_8;
    let available = (rect.width() - gap * 4.0).max(0.0);
    // The final column retains physical-motion reassignment, a real preference not shown
    // in the board. The illustration column absorbs narrow-window pressure first.
    let label = t.form_label_width + t.space_6;
    let invert = t.form_label_width;
    let speed = t.space_24 * 2.0 + t.space_16;
    let source = t.form_label_width + t.space_24 + t.space_16;
    let live = t.form_label_width * 2.0 + t.space_24 + t.space_6;
    let widths = [label, (available - label - invert - speed - source).clamp(0.0, live), invert, speed];
    let mut x = rect.left();
    std::array::from_fn(|index| {
        let width = widths.get(index).copied().unwrap_or((rect.right() - x).max(0.0));
        let column = Rect::from_min_size(pos2(x, rect.top()), vec2(width, rect.height()));
        x += width + gap;
        column
    })
}

pub(crate) fn settings_axis_header(ui: &mut Ui) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_24), Sense::hover());
    if rect.width() < t.form_label_width * 5.0 + t.space_24 * 5.0 {
        let half = rect.width() * 0.5;
        for (index, label) in ["prefs.axis", "prefs.live_input"].into_iter().enumerate() {
            let cell = Rect::from_min_size(rect.min + vec2(half * index as f32, 0.0), vec2(half, rect.height()));
            paint_text(ui, cell, &crate::i18n::tr(label).to_uppercase(), crate::fonts::semibold(t.font_small), t.form_dim, false);
        }
        return;
    }
    for (column, label) in axis_columns(rect).into_iter().zip(["prefs.axis", "prefs.live_input", "", "prefs.speed", "prefs.source"]) {
        paint_text(ui, column, &crate::i18n::tr(label).to_uppercase(), crate::fonts::semibold(t.font_small), t.form_dim, false);
    }
}

/// One table row combines measured input with the actual invert, gain, and motion controls.
pub(crate) fn settings_axis_row(
    ui: &mut Ui,
    name: &str,
    value: f32,
    dead_zone: [f32; 2],
    invert: impl FnOnce(&mut Ui),
    speed: impl FnOnce(&mut Ui),
    source: impl FnOnce(&mut Ui),
) {
    let t = Tokens::get();
    let narrow = ui.available_width() < t.form_label_width * 5.0 + t.space_24 * 5.0;
    let line = t.control_height + t.space_4;
    // A narrow row stacks the real controls instead of clipping its final source dropdown.
    let height = if narrow { line * 4.0 + t.space_8 * 3.0 } else { line };
    super::settings_form(ui, 0.0, |ui| {
        super::layout::sized_row(ui, name, height, |ui| {
            // The row already owns these bounds; its centered cursor is not the top edge.
            let rect = ui.max_rect();
            crate::scenario::publish_rect(&format!("prefs:axis:{name}"), rect);
            t.secondary_hline(ui.painter(), rect.x_range(), rect.bottom(), t.dialog_rule);
            if narrow {
                let label_rect = Rect::from_min_size(rect.min, vec2(rect.width() * 0.5, line));
                paint_text(ui, label_rect, name, FontId::proportional(t.font_label), t.form_ink, false);
                let live_rect = Rect::from_min_max(pos2(label_rect.right() + t.space_8, rect.center().y), pos2(rect.right(), rect.center().y));
                let live_rect =
                    Rect::from_center_size(pos2(live_rect.center().x, label_rect.center().y), vec2(live_rect.width().max(0.0), t.space_14));
                live_axis_bar_in(ui, live_rect, value, dead_zone);
                let cell = |index: f32| Rect::from_min_size(pos2(rect.left(), rect.top() + (line + t.space_8) * index), vec2(rect.width(), line));
                axis_control(ui, cell(1.0), "invert", name, invert);
                let label_width = super::ContentWidth::label(ui, crate::i18n::tr("prefs.speed"))
                    .max(super::ContentWidth::label(ui, crate::i18n::tr("prefs.source")));
                axis_control(ui, cell(2.0), "speed", name, |ui| {
                    super::settings_form(ui, label_width, |ui| super::row(ui, crate::i18n::tr("prefs.speed"), speed));
                });
                axis_control(ui, cell(3.0), "source", name, |ui| {
                    super::settings_form(ui, label_width, |ui| super::row(ui, crate::i18n::tr("prefs.source"), source));
                });
            } else {
                let [label_rect, live_rect, invert_rect, speed_rect, source_rect] = axis_columns(rect);
                paint_text(ui, label_rect, name, FontId::proportional(t.font_label), t.form_ink, false);
                ui.interact(label_rect, ui.id().with("axis_name"), Sense::hover()).on_hover_text(name);
                let live_rect = Rect::from_center_size(live_rect.center(), vec2(live_rect.width(), t.space_14));
                live_axis_bar_in(ui, live_rect, value, dead_zone);
                axis_control(ui, invert_rect, "invert", name, invert);
                axis_control(ui, speed_rect, "speed", name, speed);
                axis_control(ui, source_rect, "source", name, source);
            }
        });
    });
}
fn axis_control(ui: &mut Ui, rect: Rect, id: &str, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(id)
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center))
            .ui_stack_info(egui::UiStackInfo::default().with_tag_value(super::layout::LABEL, label.to_owned())),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(rect));
            ui.spacing_mut().text_edit_width = rect.width();
            add(ui);
        },
    );
}

/// Paint a calibrated live-input bar into an existing table cell or gallery slot.
pub(crate) fn live_axis_bar_in(ui: &Ui, live_rect: Rect, value: f32, dead_zone: [f32; 2]) {
    let t = Tokens::get();
    let p = ui.painter().with_clip_rect(live_rect.expand(t.space_2));
    p.rect_filled(live_rect, t.radius_lg, t.dialog_rule);
    let [negative, positive] = dead_zone.map(|share| if share.is_finite() { share.clamp(0.0, 1.0) } else { 0.0 });
    let zone = Rect::from_min_max(
        pos2(live_rect.center().x - live_rect.width() * 0.5 * negative, live_rect.top()),
        pos2(live_rect.center().x + live_rect.width() * 0.5 * positive, live_rect.bottom()),
    );
    let mut x = zone.left() - zone.height();
    while x < zone.right() {
        p.with_clip_rect(zone).line_segment([pos2(x, zone.bottom()), pos2(x + zone.height(), zone.top())], t.control_border_stroke(t.control_border));
        x += t.space_4;
    }
    let value = if value.is_finite() { value.clamp(-1.0, 1.0) } else { 0.0 };
    let end = live_rect.center().x + live_rect.width() * 0.5 * value;
    p.rect_filled(
        Rect::from_x_y_ranges(
            live_rect.center().x.min(end)..=live_rect.center().x.max(end),
            live_rect.top() + t.space_2..=live_rect.bottom() - t.space_2,
        ),
        t.radius_lg,
        t.form_primary,
    );
    p.vline(live_rect.center().x, live_rect.top() - t.space_2..=live_rect.bottom() + t.space_2, Stroke::new(1.0, t.form_dim));
}

/// Compact per-axis gain uses a consistent two-decimal specimen and the real accepted range.
pub(crate) fn settings_axis_gain(ui: &mut Ui, gain: &mut f32) -> egui::Response {
    if crate::prefs_search::querying(ui) && (!ui.is_visible() || crate::prefs_search::suppress_chrome(ui)) {
        return crate::prefs_search::hidden_response(ui);
    }
    let response =
        ui.add(egui::DragValue::new(gain).range(0.0..=3.0).speed(0.05).suffix("×").fixed_decimals(2).custom_parser(crate::prefs::parse_number));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::drag_value(ui.is_enabled(), f64::from(*gain));
        info.label = Some(super::layout::label(ui).to_owned());
        info
    });
    super::layout::publish_geometry(ui, &response);
    response
}
