//! Compact navigation preference compositions and explanatory figures.
use super::{metrics::dimension, text::paint_text};
use crate::theme::Tokens;
use crate::ui_kit::BackingTooltipExt as _;
use egui::{FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

/// One mouse silhouette beside the actual mapping selector and binding legend.
/// The figure is descriptive content, not an icon asset or an application viewport.
pub(crate) fn settings_mouse_mapping(ui: &mut Ui, bindings: &[(Vec<String>, String)], mapping: impl FnOnce(&mut Ui)) {
    if crate::prefs_search::suppress_chrome(ui) {
        mapping(ui);
        for (keys, action) in bindings.iter().take(10) {
            let chord = keys.join(" ");
            if crate::prefs_search::texts_hit(ui, &[action, &chord]) {
                crate::prefs_search::reveal(ui);
                settings_gesture(ui, keys, action);
            }
        }
        return;
    }
    let t = Tokens::get();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_24 - t.space_2;
        let figure_size = vec2(t.form_label_width + t.space_24 + t.space_16, t.space_24 * 6.0 + t.space_16);
        let (figure, _) = ui.allocate_exact_size(figure_size, Sense::hover());
        let width = dimension(ui.available_width());
        let content = ui.allocate_ui_with_layout(vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            mapping(ui);
            ui.add_space(t.space_6);
            for (keys, action) in bindings.iter().take(10) {
                settings_gesture(ui, keys, action);
            }
        });
        let height = figure.height().max(content.response.rect.height());
        let figure = Rect::from_min_size(figure.min + vec2(0.0, (height - figure.height()) * 0.5), figure_size);
        let point = |x: f32, y: f32| figure.min + vec2(figure.width() * x / 110.0, figure.height() * y / 160.0);
        let mouse = Rect::from_min_max(point(12.0, 6.0), point(98.0, 154.0));
        let p = ui.painter();
        p.rect_filled(mouse, mouse.width() * 0.5, t.control_fill);
        let left = Rect::from_min_max(mouse.min, pos2(mouse.center().x, point(0.0, 68.0).y));
        p.with_clip_rect(left).rect_filled(mouse, mouse.width() * 0.5, t.selected_bg);
        let right = Rect::from_min_max(pos2(mouse.center().x, mouse.top()), pos2(mouse.right(), point(0.0, 68.0).y));
        p.with_clip_rect(right).rect_filled(mouse, mouse.width() * 0.5, t.warning.gamma_multiply(0.18));
        p.rect_stroke(mouse, mouse.width() * 0.5, Stroke::new(1.5, t.control_border), egui::StrokeKind::Inside);
        p.vline(mouse.center().x, mouse.top()..=point(0.0, 68.0).y, Stroke::new(1.2, t.control_border));
        p.hline(mouse.x_range(), point(0.0, 68.0).y, Stroke::new(1.2, t.control_border));
        p.rect_filled(Rect::from_min_max(point(48.0, 24.0), point(62.0, 54.0)), t.radius_lg, t.form_primary);
    });
}

/// Keyboard and pointer gestures use the same quiet keycap treatment as shortcuts.
fn settings_gesture(ui: &mut Ui, keys: &[String], action: &str) {
    let t = Tokens::get();
    let width = dimension(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.control_height + t.space_2), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), format!("{}: {action}", keys.join(" + "))));
    t.secondary_hline(ui.painter(), rect.x_range(), rect.bottom(), t.dialog_rule);
    let key_width = (t.form_label_width * 2.0 + t.space_24 + t.space_6).min(width * 0.57);
    let key_rect = Rect::from_min_size(rect.min, vec2(key_width, rect.height()));
    let p = ui.painter().with_clip_rect(key_rect);
    let mut x = key_rect.left();
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            let join = Rect::from_min_size(pos2(x, rect.top()), vec2(t.space_12, rect.height()));
            paint_text(ui, join, "+", FontId::proportional(t.font_small), t.form_dim, true);
            x += t.space_12;
        }
        let galley = p.layout_no_wrap(key.to_owned(), FontId::proportional(t.font_small), t.form_ink);
        let cap = Rect::from_center_size(
            pos2(x + (galley.size().x + t.space_12) / 2.0, rect.center().y),
            vec2(galley.size().x + t.space_12, t.space_24 - t.space_2),
        );
        p.rect(cap, t.radius_sm, t.control_fill, t.container_border(t.control_border), egui::StrokeKind::Inside);
        p.galley(pos2(cap.center().x - galley.size().x / 2.0, cap.center().y - galley.size().y / 2.0), galley, t.form_ink);
        x = cap.right() + t.space_4;
    }
    let action_rect = Rect::from_min_max(pos2(key_rect.right() + t.space_8, rect.top()), rect.max);
    paint_text(ui, action_rect, action, FontId::proportional(t.font_label), t.form_ink, false);
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
    if crate::prefs_search::querying(ui) && crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[name]) {
        crate::prefs_search::run_hidden(ui, name, |ui| {
            invert(ui);
            speed(ui);
            source(ui);
        });
        return;
    }
    if crate::prefs_search::querying(ui) {
        crate::prefs_search::reveal(ui);
    }
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.control_height + t.space_4), Sense::hover());
    t.secondary_hline(ui.painter(), rect.x_range(), rect.bottom(), t.dialog_rule);
    let [label_rect, live_rect, invert_rect, speed_rect, source_rect] = axis_columns(rect);
    paint_text(ui, label_rect, name, FontId::proportional(t.font_label), t.form_ink, false);
    ui.interact(label_rect, ui.id().with("axis_name"), Sense::hover()).backing_on_hover_text(name);
    let live_rect = Rect::from_center_size(live_rect.center(), vec2(live_rect.width(), t.space_14));
    live_axis_bar_in(ui, live_rect, value, dead_zone);
    axis_control(ui, invert_rect, "invert", name, invert);
    axis_control(ui, speed_rect, "speed", name, speed);
    axis_control(ui, source_rect, "source", name, source);
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
