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

/// Compact completion readout for a strip. The caller owns the result identity.
/// Tokens: font_small, form_ink/success/dim, control_height and space_4.
pub(crate) fn completion_text(ui: &mut egui::Ui, label: &str, complete: bool) -> egui::Response {
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_small);
    let text = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), t.form_ink);
    let tick_width = if complete { t.space_14 + t.space_4 } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(text.size().x + tick_width, t.control_height), egui::Sense::hover());
    if complete {
        super::checks::paint_check_glyph(
            ui.painter(),
            egui::Rect::from_center_size(rect.left_center() + vec2(t.space_14 / 2.0, 0.0), vec2(t.space_14, t.space_14)),
            false,
            t.form_success,
        );
    }
    super::text::paint_text(
        ui,
        egui::Rect::from_min_max(rect.min + vec2(tick_width, 0.0), rect.max),
        label,
        font,
        if complete { t.form_ink } else { t.form_dim },
        false,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), label));
    response
}

/// Read-only check row with shared success/warning artwork and token-sized icon.
pub(crate) fn check_line(ui: &mut egui::Ui, key: &str, label: &str, warning: bool) -> egui::Response {
    check_status_line(ui, key, label, if warning { CheckStatus::Warning } else { CheckStatus::Ok })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CheckStatus {
    Ok,
    Warning,
    Error,
}

impl CheckStatus {
    /// Plain-text marker for copied check reports.
    pub(crate) fn text_marker(self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Warning => "!",
            Self::Error => "×",
        }
    }
}

/// Semantic status glyphs use the same geometry and theme colours in every report.
pub(crate) fn check_status_line(ui: &mut egui::Ui, key: &str, label: &str, status: CheckStatus) -> egui::Response {
    let t = Tokens::get();
    let response = ui
        .horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(t.form_icon_size, t.form_icon_size), egui::Sense::hover());
            match status {
                CheckStatus::Ok => super::checks::paint_check_glyph(ui.painter(), rect, false, t.form_success),
                CheckStatus::Warning | CheckStatus::Error => {
                    let ink = if status == CheckStatus::Warning { t.warning } else { t.form_error };
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        if status == CheckStatus::Warning { "!" } else { "×" },
                        egui::FontId::proportional(t.font_subtitle),
                        ink,
                    );
                }
            }
            super::text(ui, label, super::Text::Report(key, false));
        })
        .response;
    crate::scenario::publish_rect(key, response.rect);
    response
}

/// Single-row render status for a shared Navigation card. The caller dispatches Cancel.
pub(crate) fn progress_action(ui: &mut egui::Ui, caption: &str, fraction: f32, cancel: &str) -> egui::Response {
    let t = Tokens::get();
    ui.horizontal(|ui| {
        super::text(ui, caption, super::Text::Inline);
        ui.allocate_ui_with_layout(vec2(t.space_24 * 6.0, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            progress(ui, "render-pill", fraction);
        });
        super::button(ui, cancel, super::Button::Normal)
    })
    .inner
}
