//! Compact navigation preference compositions and explanatory figures.
use super::metrics::dimension;
pub use super::preferences_figure_data::{paint_orbit_centre, paint_orbit_cursor, paint_orbit_origin};
pub use super::preferences_figure_ops::{NavigatorAxis, paint_navigation_axes, paint_navigation_custom};
use egui::{Rect, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

/// One mouse silhouette beside the actual mapping selector and binding legend.
/// The figure is descriptive content, not an icon asset or an application viewport.
pub fn settings_mouse_mapping(ui: &mut Ui, bindings: &[(Vec<String>, String)], mapping: impl FnOnce(&mut Ui)) {
    let legend = |ui: &mut Ui| {
        // Registry identities only: this legend omits the opt-in table headers.
        let columns = [("input", 1.5), ("action", 1.0)].map(|(id, weight)| super::Column::new(id, weight).flex());
        super::table_fitted(ui, "mouse_mapping", &columns, "", |ui, table| {
            for (index, (keys, action)) in bindings.iter().enumerate() {
                let chord = keys.join(" + ");
                if super::settings_search::suppress_chrome(ui) && !super::settings_search::texts_hit(ui, &[action, &chord]) {
                    continue;
                }
                if super::settings_search::querying(ui) && !super::settings_search::shows_all(ui) {
                    super::settings_search::reveal(ui);
                }
                super::settings_search::push_passthrough(ui);
                table.row(ui, &index.to_string(), false, |row| {
                    row.cell(|ui| super::shortcut_binding(ui, &keys.join("+"), "", false));
                    row.cell(|ui| super::text(ui, action, super::Text::Cell));
                });
                super::settings_search::pop_passthrough(ui);
            }
        });
    };
    if super::settings_search::suppress_chrome(ui) {
        mapping(ui);
        legend(ui);
        return;
    }
    let t = Tokens::get();
    mapping(ui);
    ui.add_space(t.space_8);
    let available = dimension(ui.available_width());
    let figure_size = vec2(t.space_24 * 5.0, t.space_24 * 6.0);
    let gap = t.space_24 - t.space_2;
    let beside = available >= figure_size.x + gap + t.space_24 * 12.0;
    let content_width = if beside { available - figure_size.x - gap } else { available };
    ui.with_layout(egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let (figure, _) = ui.allocate_exact_size(vec2(figure_size.x.min(available), figure_size.y), Sense::hover());
        let content = ui.allocate_ui_with_layout(vec2(content_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            legend(ui);
        });
        let centre_y = if beside { content.response.rect.center().y } else { figure.center().y };
        let figure = Rect::from_center_size(pos2(figure.center().x, centre_y), figure.size());
        super::preferences_figure_ops::paint_mouse(&ui.painter_at(figure), figure);
    });
}

/// Paint a calibrated live-input bar into an existing table cell or gallery slot.
pub fn live_axis_bar_in(ui: &Ui, live_rect: Rect, value: f32, dead_zone: [f32; 2]) {
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
pub fn settings_axis_gain(ui: &mut Ui, gain: &mut f32) -> egui::Response {
    if super::settings_search::querying(ui) && (!ui.is_visible() || super::settings_search::suppress_chrome(ui)) {
        return super::settings_search::hidden_response(ui);
    }
    let response = ui.add(egui::DragValue::new(gain).range(0.0..=3.0).speed(0.05).suffix("×").fixed_decimals(2).custom_parser(super::parse_number));
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::drag_value(ui.is_enabled(), f64::from(*gain));
        info.label = Some(super::layout::label(ui).to_owned());
        info
    });
    super::layout::publish_geometry(ui, &response);
    response
}
