//! Read-only report and library containers, sharing the form's typography and actions.
use crate::{scenario, theme::Tokens};
use egui::{Ui, vec2};

/// Bordered list with a bounded scroll viewport. Tokens: dialog_rule, radius_md, space_10.
pub(crate) fn report_list<R>(ui: &mut Ui, key: &str, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    list_frame(ui, key, |ui| {
        let output = egui::ScrollArea::vertical()
            .id_salt(("report_list", key))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .content_margin(egui::Margin::ZERO)
            .auto_shrink([false, true])
            .max_height(height)
            .show(ui, add);
        scenario::publish_rect(&format!("dialog:list:{key}:viewport"), output.inner_rect);
        scenario::publish_count(&format!("dialog:list:{key}:content_width"), f64::from(output.content_size.x));
        output.inner
    })
}

/// Full-width report hairline across the list's text inset. Tokens: space_10, dialog_rule.
pub(crate) fn report_divider(ui: &mut Ui, key: &str) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), 1.0), egui::Sense::hover());
    let rect = rect.expand2(vec2(t.space_10, 0.0));
    let clip = ui.clip_rect().expand2(vec2(t.space_10, 0.0));
    ui.painter().with_clip_rect(clip).hline(rect.x_range(), rect.top(), t.tertiary_border(t.dialog_rule));
    scenario::publish_rect(&format!("dialog:divider:{key}"), rect);
}

/// List border around a caller-owned virtualised viewport. Tokens: dialog_rule, control_radius, space_10.
pub(crate) fn list_frame<R>(ui: &mut Ui, key: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    let response = egui::Frame::NONE
        .stroke(t.container_border(t.dialog_rule))
        .corner_radius(t.control_radius)
        .inner_margin(egui::Margin::symmetric(t.space_10 as i8, 0))
        .show(ui, |ui| {
            // Frame already subtracts its border and padding from the content width.
            ui.set_min_width(ui.available_width().max(0.0));
            add(ui)
        });
    scenario::publish_rect(&format!("dialog:list:{key}"), response.response.rect);
    response.inner
}

#[cfg(test)]
#[path = "reports_tests.rs"]
mod tests;

/// Read-only monospace path chip with an adjacent action. Retains the path cell handle.
pub(crate) fn path_chip(ui: &mut Ui, path: &str, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_8;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            add(ui);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), t.control_height), egui::Sense::hover());
            ui.painter().rect(rect, t.control_radius, t.dialog_footer, t.tertiary_border(t.dialog_rule), egui::StrokeKind::Inside);
            super::text::paint_text(ui, rect.shrink2(vec2(t.space_8, 0.0)), path, egui::FontId::monospace(t.font_small), t.form_dim, false);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), path));
            response.on_hover_text(path);
            let key = format!("dialog:cell:{path}");
            scenario::publish_handle(&key, rect.center());
            scenario::publish_rect(&key, rect);
        });
    });
}

/// Empty-list dashed outline and centred caption. Retains the note rectangle.
pub(crate) fn empty_box(ui: &mut Ui, key: &str, message: &str) {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), t.control_height * 3.0), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), message));
    let mut points = super::preferences::rounded_perimeter(rect.shrink(0.5), f32::from(t.radius_lg));
    if let Some(first) = points.first().copied() {
        points.push(first);
    }
    ui.painter().extend(egui::Shape::dashed_line(&points, t.control_border_stroke(t.control_border), t.space_4, t.space_3));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, message, egui::FontId::proportional(t.font_label), t.form_dim);
    scenario::publish_rect(&format!("dialog:note:{key}"), rect);
}

/// Kind/count above UTC time, alongside actions. The legacy combined-title cell remains an alias.
pub(crate) fn report_header(ui: &mut Ui, kind: &str, count: u64, time: &str, alias: &str, actions: impl FnOnce(&mut Ui)) {
    let t = Tokens::get();
    let height = t.control_height + t.space_8;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            actions(ui);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), height), egui::Sense::hover());
            let title = egui::Rect::from_min_size(rect.min, vec2(rect.width(), height / 2.0));
            let count_text = if count > 1 { format!("×{count}") } else { String::new() };
            let pill_width = if count > 1 {
                ui.painter().layout_no_wrap(count_text.clone(), egui::FontId::proportional(t.font_small), t.form_dim).size().x + t.space_12
            } else {
                0.0
            };
            let kind_width = ui.painter().layout_no_wrap(kind.to_string(), crate::fonts::semibold(t.font_label), t.form_ink).size().x;
            let end = (title.left() + kind_width).min((title.right() - pill_width - t.space_6).max(title.left()));
            super::text::paint_text(
                ui,
                egui::Rect::from_min_max(title.min, egui::pos2(end, title.bottom())),
                kind,
                crate::fonts::semibold(t.font_label),
                t.form_ink,
                false,
            );
            if count > 1 {
                let pill = egui::Rect::from_min_size(egui::pos2(end + t.space_6, title.top()), vec2(pill_width, title.height()));
                ui.painter().rect_filled(pill, t.control_radius, t.dialog_footer);
                ui.painter().text(pill.center(), egui::Align2::CENTER_CENTER, count_text, egui::FontId::proportional(t.font_small), t.form_dim);
            }
            super::text::paint_text(
                ui,
                egui::Rect::from_min_max(egui::pos2(rect.left(), title.bottom()), rect.max),
                time,
                egui::FontId::monospace(t.font_small),
                t.form_dim,
                false,
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), alias));
            response.on_hover_text(alias);
            let key = format!("dialog:cell:{alias}");
            scenario::publish_rect(&key, rect);
            scenario::publish_handle(&key, rect.center());
        });
    });
}
