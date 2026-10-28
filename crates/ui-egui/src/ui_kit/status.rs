//! Status heading and constraint meter; Tokens: font_label, text, text_dim, accent, dialog_rule, sketch_accent, spacing.
use crate::{fonts::RichTextExt as _, theme::Tokens};
use egui::{RichText, vec2};

/// Status content for a named card. `key` is the scenario namespace, `fraction` is clamped.
pub(crate) struct StatusHeader<'a> {
    pub icon: &'a str,
    pub text: &'a str,
    pub complete: bool,
    pub fraction: f32,
}

/// Card identity, status dot/text and progress meter; use for solver/selection state.
/// Tokens: font_label, text/dim, accent, dialog_rule, sketch_accent, form_icon_size, spacing.
/// Name, status dot/text and meter for a card. Tokens: `font_label, text/dim, accent, dialog_rule, sketch_accent, form_icon_size, space_3/4/6/8/10`.
pub(crate) fn status_header(ui: &mut egui::Ui, key: &str, name: &str, status: StatusHeader<'_>) -> egui::Response {
    let fraction = if status.fraction.is_finite() { status.fraction.clamp(0.0, 1.0) } else { 0.0 };
    let t = Tokens::get();
    let response = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(t.space_10 as i8, t.space_8 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(t.space_4, t.space_4);
            ui.spacing_mut().interact_size.y = t.form_icon_size;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(t.form_icon_size, t.form_icon_size), egui::Sense::hover());
                crate::icons::paint_icon(ui.painter(), rect, status.icon, f32::from(t.icon.a()) / 255.0);
                ui.add(egui::Label::new(RichText::new(name).semibold().size(t.font_label)).truncate()).on_hover_text(name);
            });
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(t.space_10, t.space_10), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), t.space_10 / 2.0, if status.complete { t.form_ink } else { t.form_primary });
                ui.add(egui::Label::new(RichText::new(status.text).size(t.font_label).color(t.form_dim)).wrap());
            });
            let (meter, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.space_6), egui::Sense::hover());
            ui.painter().rect_filled(meter, t.space_3, t.dialog_rule);
            if fraction > 0.0 {
                ui.painter().rect_filled(egui::Rect::from_min_size(meter.min, vec2(meter.width() * fraction, meter.height())), t.space_3, t.form_ink);
            }
            crate::scenario::publish_rect(&format!("{key}:meter"), meter);
            crate::scenario::publish_count(&format!("{key}:constrained_fraction"), f64::from(fraction));
        })
        .response;
    t.secondary_hline(ui.painter(), ui.max_rect().x_range(), ui.cursor().top(), t.dialog_rule);
    crate::scenario::publish_target(&format!("{key}:status_header"), response.rect);
    response
}

/// Determinate progress bar for a running operation (a download, a file load), full width and
/// `space_6` tall; `fraction` is clamped (non-finite counts as zero). Pair it with a `text_block`
/// that names the step. Tokens: `space_3/6, dialog_rule, accent`.
pub(crate) fn progress(ui: &mut egui::Ui, key: &str, fraction: f32) -> egui::Response {
    let t = Tokens::get();
    let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let (bar, response) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), t.space_6), egui::Sense::hover());
    ui.painter().rect_filled(bar, t.space_3, t.dialog_rule);
    if fraction > 0.0 {
        ui.painter().rect_filled(egui::Rect::from_min_size(bar.min, vec2(bar.width() * fraction, bar.height())), t.space_3, t.form_primary);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ProgressIndicator, ui.is_enabled(), format!("{:.0}%", fraction * 100.0)));
    crate::scenario::publish_rect(&format!("dialog:progress:{key}"), bar);
    response
}

/// Steps of a running operation, one row each: a tick for the ones done, a spinner for the one
/// under way and a ring for the ones to come, each the height of the text and centred on it.
/// `detail` (how far the current step is) goes at the end of its row. Pair it with `progress`
/// for the current step. Tokens: `font_label, control_height, form_ink/dim/primary, space_2/8`.
pub(crate) fn steps(ui: &mut egui::Ui, key: &str, labels: &[&str], current: usize, detail: Option<&str>) {
    let t = Tokens::get();
    let mark = t.font_label;
    for (i, label) in labels.iter().enumerate() {
        let (row, response) =
            ui.allocate_exact_size(vec2(super::metrics::dimension(ui.available_width()), super::row_height()), egui::Sense::hover());
        let slot = egui::Rect::from_center_size(egui::pos2(row.left() + mark / 2.0, row.center().y), vec2(mark, mark));
        let painter = ui.painter();
        let (font, ink) = match i.cmp(&current) {
            std::cmp::Ordering::Less => {
                super::checks::paint_check_glyph(painter, slot, false, t.form_ink);
                (egui::FontId::proportional(t.font_label), t.form_ink)
            }
            std::cmp::Ordering::Equal => {
                egui::Spinner::new().size(mark).color(t.form_primary).paint_at(ui, slot);
                (crate::fonts::semibold(t.font_label), t.form_ink)
            }
            std::cmp::Ordering::Greater => {
                painter.circle_stroke(slot.center(), mark / 2.0 - t.space_2, egui::Stroke::new(1.0, t.form_dim));
                (egui::FontId::proportional(t.font_label), t.form_dim)
            }
        };
        let mut text = egui::Rect::from_min_max(egui::pos2(slot.right() + t.space_8, row.top()), row.max);
        if let Some(detail) = detail.filter(|_| i == current) {
            let galley = ui.painter().layout_no_wrap(detail.to_string(), egui::FontId::proportional(t.font_label), t.form_dim);
            let at = egui::pos2(row.right() - galley.size().x, row.center().y - galley.size().y / 2.0);
            if at.x > text.left() + t.space_8 {
                ui.painter().galley(at, galley, t.form_dim);
                text.max.x = at.x - t.space_8;
            }
        }
        super::text::paint_text(ui, text, label, font, ink, false);
        let state = match i.cmp(&current) {
            std::cmp::Ordering::Less => "done",
            std::cmp::Ordering::Equal => "current",
            std::cmp::Ordering::Greater => "next",
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), *label));
        crate::scenario::publish_rect(&format!("dialog:step:{key}:{i}:{state}"), row);
    }
}
