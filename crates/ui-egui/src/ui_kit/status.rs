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
/// under way and a ring for the ones to come. The three marks share one box the height of the
/// text, centred on its capitals, and one stroke. `detail` (how far the current step is) goes at
/// the end of its row. Pair it with `progress` for the current step.
/// Tokens: `font_label, control_height, form_ink/dim/primary, space_2/3/8`.
pub(crate) fn steps(ui: &mut egui::Ui, key: &str, labels: &[&str], current: usize, detail: Option<&str>) {
    let t = Tokens::get();
    let mark = t.font_label;
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let snap = |v: f32| (v * ppp).round() / ppp;
    for (i, label) in labels.iter().enumerate() {
        let (row, response) =
            ui.allocate_exact_size(vec2(super::metrics::dimension(ui.available_width()), super::row_height()), egui::Sense::hover());
        let order = i.cmp(&current);
        let (font, ink) = match order {
            std::cmp::Ordering::Equal => (crate::fonts::semibold(t.font_label), t.form_ink),
            std::cmp::Ordering::Less => (egui::FontId::proportional(t.font_label), t.form_ink),
            std::cmp::Ordering::Greater => (egui::FontId::proportional(t.font_label), t.form_dim),
        };
        let text_left = row.left() + mark + t.space_8;
        let mut text_right = row.right();
        if let Some(detail) = detail.filter(|_| i == current) {
            let galley = ui.painter().layout_no_wrap(detail.to_string(), egui::FontId::proportional(t.font_label), t.form_dim);
            let at = egui::pos2(snap(row.right() - galley.size().x), snap(row.center().y - galley.size().y / 2.0));
            if at.x > text_left + t.space_8 {
                text_right = at.x - t.space_8;
                ui.painter().galley(at, galley, t.form_dim);
            }
        }
        let mut job = egui::text::LayoutJob::single_section(label.to_string(), egui::TextFormat::simple(font, ink));
        job.wrap.max_width = (text_right - text_left).max(0.0);
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('\u{2026}');
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        let at = egui::pos2(snap(text_left), snap(row.center().y - galley.size().y / 2.0));
        // The mark is centred on the label's capitals (its first glyph's ink), not on the line box.
        let caps = galley
            .rows
            .first()
            .and_then(|r| r.row.glyphs.first().map(|g| r.pos.y + g.pos.y + g.uv_rect.offset.y + g.uv_rect.size.y / 2.0))
            .map_or(row.center().y, |y| at.y + y);
        let slot = egui::Rect::from_center_size(egui::pos2(row.left() + mark / 2.0, caps), vec2(mark, mark));
        let inner = slot.shrink(t.space_3);
        let stroke = egui::Stroke::new(t.space_2, ink);
        let painter = ui.painter();
        match order {
            std::cmp::Ordering::Less => super::checks::paint_check_glyph(painter, slot, false, t.form_ink),
            std::cmp::Ordering::Equal => {
                // Three quarters of a circle turning, the tick's size and stroke: a whole round, never clipped.
                // The tick's stroke is centred on the box's edges, and so is the circle's.
                let radius = inner.height() / 2.0;
                let start = ui.input(|input| input.time) as f32 * std::f32::consts::TAU * 0.8;
                let points: Vec<egui::Pos2> = (0..=24)
                    .map(|k| start + k as f32 / 24.0 * std::f32::consts::TAU * 0.75)
                    .map(|a| inner.center() + radius * vec2(a.cos(), a.sin()))
                    .collect();
                painter.add(egui::Shape::line(points, egui::Stroke::new(stroke.width, t.form_primary)));
                ui.ctx().request_repaint();
            }
            std::cmp::Ordering::Greater => {
                painter.circle_stroke(inner.center(), inner.height() / 2.0, egui::Stroke::new(1.0, t.form_dim));
            }
        }
        ui.painter().with_clip_rect(ui.clip_rect().intersect(row)).galley(at, galley, ink);
        let state = match order {
            std::cmp::Ordering::Less => "done",
            std::cmp::Ordering::Equal => "current",
            std::cmp::Ordering::Greater => "next",
        };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), *label));
        crate::scenario::publish_rect(&format!("dialog:step:{key}:{i}:{state}"), row);
        crate::scenario::publish_rect(&format!("dialog:step_mark:{key}:{i}"), slot);
    }
}
