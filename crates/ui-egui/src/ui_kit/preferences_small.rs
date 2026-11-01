//! Shared labelled settings blocks and compact explanatory specimens.
use crate::theme::Tokens;
use egui::{Align, FontId, Layout, Rect, Sense, Stroke, Ui, pos2, vec2};

/// A labelled form block whose control column can contain several natural-height rows.
pub(crate) fn settings_block<R>(ui: &mut Ui, label: &str, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    settings_block_content(ui, label, None, label_width, add)
}

/// A form block whose explanatory hint belongs below its label, beside the controls.
pub(crate) fn settings_block_hint<R>(ui: &mut Ui, label: &str, hint: &str, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    settings_block_content(ui, label, Some(hint), label_width, add)
}

fn settings_block_content<R>(ui: &mut Ui, label: &str, hint: Option<&str>, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    if let Some(gate) = crate::prefs_search::block_gate(ui, label, hint) {
        let before = crate::prefs_search::hits(ui);
        if !gate.show {
            let result = crate::prefs_search::run_hidden(ui, label, add);
            crate::prefs_search::block_finish(ui, &gate, false, before);
            return result;
        }
        let result = settings_block_body(ui, label, hint, label_width, add);
        crate::prefs_search::block_finish(ui, &gate, true, before);
        return result;
    }
    settings_block_body(ui, label, hint, label_width, add)
}

fn settings_block_body<R>(ui: &mut Ui, label: &str, hint: Option<&str>, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    let width = super::metrics::dimension(ui.available_width());
    let label_width = label_width.min(width * 0.45);
    let response = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_14;
        ui.allocate_ui_with_layout(vec2(label_width, t.control_height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(label_width);
            ui.spacing_mut().item_spacing.y = t.space_2;
            ui.add_space(t.form_gap_y);
            super::text(ui, label, super::Text::Inline);
            if let Some(hint) = hint {
                super::text(ui, hint, super::Text::Hint(label));
            }
        });
        let control_width = (width - label_width - t.space_14).max(0.0);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(ui.next_widget_position(), vec2(control_width, 0.0)))
                .layout(Layout::top_down(Align::Min))
                .ui_stack_info(egui::UiStackInfo::default().with_tag_value(super::layout::LABEL, label.to_string())),
            |ui| {
                ui.set_width(control_width);
                ui.spacing_mut().text_edit_width = control_width;
                add(ui)
            },
        )
        .inner
    });
    crate::scenario::publish_rect(&format!("dialog:row:{label}"), response.response.rect);
    response.inner
}

/// A bounded control that keeps its intended width inside a larger form column.
pub(crate) fn settings_control_width<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = super::metrics::dimension(width).min(super::metrics::dimension(ui.available_width()));
    ui.allocate_ui_with_layout(vec2(width, Tokens::get().control_height), Layout::left_to_right(Align::Center), add).inner
}

/// A small numeric control, using the same number editor and geometry as every form.
pub(crate) fn settings_number<T: egui::emath::Numeric>(ui: &mut Ui, value: &mut T, range: std::ops::RangeInclusive<T>, speed: f64) -> egui::Response {
    ui.scope(|ui| {
        let t = Tokens::get();
        ui.spacing_mut().interact_size.x = t.form_label_width - t.space_6;
        super::number_field(ui, value, range, speed, "")
    })
    .inner
}

/// An illustrative half-hour of autosave marks, using the actual configured interval.
pub(crate) fn settings_autosave_timeline(ui: &mut Ui, minutes: f64) {
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(super::metrics::dimension(ui.available_width()), t.space_24 + t.space_12 + t.space_2), Sense::hover());
    let line = r;
    ui.painter().hline(line.x_range(), r.top() + t.space_10, Stroke::new(2.0, t.dialog_rule));
    if minutes.is_finite() && minutes > 0.0 {
        let ticks = (30.0 / minutes).floor().clamp(0.0, 60.0) as u32;
        for i in 0..=ticks {
            let x = line.right() - line.width() * (f64::from(i) * minutes / 30.0) as f32;
            ui.painter().vline(x, r.top() + t.space_4..=r.top() + t.space_16, Stroke::new(2.0, t.form_primary));
        }
    }
    for (share, key) in [(0.0, "prefs.timeline_half_hour"), (0.5, "prefs.timeline_quarter_hour"), (1.0, "prefs.timeline_now")] {
        ui.painter().text(
            pos2(line.left() + line.width() * share, r.top() + t.space_24),
            if share == 0.0 {
                egui::Align2::LEFT_TOP
            } else if share == 1.0 {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::CENTER_TOP
            },
            crate::i18n::tr(key),
            FontId::proportional(t.font_small),
            t.form_dim,
        );
    }
}

/// A keyboard binding table: keycaps are separate from actions and never imply fixed bindings.
pub(crate) fn settings_bindings(ui: &mut Ui, bindings: &[(&[&str], &str)]) {
    if crate::prefs_search::querying(ui) && !crate::prefs_search::shows_all(ui) {
        return;
    }
    let t = Tokens::get();
    for (keys, action) in bindings {
        let width = super::metrics::dimension(ui.available_width());
        let (r, _) = ui.allocate_exact_size(vec2(width, t.control_height + t.space_2), Sense::hover());
        t.secondary_hline(ui.painter(), r.x_range(), r.bottom(), t.dialog_rule);
        let mut left = r.left();
        for key in *keys {
            let galley = ui.painter().layout_no_wrap((*key).into(), FontId::proportional(t.font_small), t.form_ink);
            let key_rect = Rect::from_center_size(
                pos2(left + (galley.size().x + t.space_12) * 0.5, r.center().y),
                vec2(galley.size().x + t.space_12, t.control_height - t.space_8),
            );
            ui.painter().rect(key_rect, t.radius_sm, t.control_fill, t.container_border(t.control_border), egui::StrokeKind::Inside);
            ui.painter().galley(key_rect.center() - galley.size() * 0.5, galley, t.form_ink);
            left = key_rect.right() + t.space_4;
        }
        let text_rect = Rect::from_min_max(pos2(r.left() + (t.space_24 * 8.0 + t.space_8).min(width * 0.45) + t.space_10, r.top()), r.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::left_to_right(Align::Center)), |ui| {
            super::text(ui, action, super::Text::Inline);
        });
    }
}

/// Natural-width, dim monospaced numeric specimen beside a precision control.
pub(crate) fn settings_precision_sample(ui: &mut Ui, text: &str) {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::monospace(t.font_body), t.form_dim);
    let (r, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    ui.painter().galley(r.min, galley, t.form_dim);
}
