//! Shared concise choice cards and the agent-pointer settings specimen.
use super::{metrics::dimension, text::paint_text};
use crate::ui_kit::BackingTooltipExt as _;
use crate::{scenario, theme::Tokens};
use egui::{FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

/// One concise choice with its full meaning available to keyboard users and on hover.
pub(crate) struct SettingsChoice<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub summary: &'a str,
    pub detail: &'a str,
}

/// Equal option cards keep the choices comparable without repeating long explanations.
pub(crate) fn settings_choices(ui: &mut Ui, choices: &[SettingsChoice<'_>], selected: &mut usize) -> bool {
    if crate::prefs_search::querying(ui) && !crate::prefs_search::shows_all(ui) {
        let keep: Vec<usize> = choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| crate::prefs_search::texts_hit(ui, &[choice.title, choice.summary]))
            .map(|(index, _)| index)
            .collect();
        if keep.is_empty() {
            return false;
        }
        crate::prefs_search::reveal(ui);
        return choice_row(ui, choices, selected, &keep);
    }
    let keep: Vec<usize> = (0..choices.len()).collect();
    choice_row(ui, choices, selected, &keep)
}

fn choice_row(ui: &mut Ui, choices: &[SettingsChoice<'_>], selected: &mut usize, keep: &[usize]) -> bool {
    let t = Tokens::get();
    let count = keep.len().max(1);
    let width = ((dimension(ui.available_width()) - t.space_8 * count.saturating_sub(1) as f32) / count as f32).max(0.0);
    let mut changed = false;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_8;
        for index in keep {
            let Some(choice) = choices.get(*index) else { continue };
            let response = choice_card(ui, width, choice, *selected == *index, false);
            if response.clicked() {
                *selected = *index;
                changed = true;
            }
        }
    });
    changed
}

/// Compact radio tile for preset lists, sharing the settings choice painter and tokens.
pub(crate) fn settings_choice_row(ui: &mut Ui, choice: &SettingsChoice<'_>, selected: bool) -> Response {
    choice_card(ui, dimension(ui.available_width()), choice, selected, true)
}
fn choice_card(ui: &mut Ui, width: f32, choice: &SettingsChoice<'_>, selected: bool, row: bool) -> Response {
    let t = Tokens::get();
    let accent = crate::theme::TabAccent::current(ui.ctx());
    let selected_bg = if row { accent.soft } else { t.selected_bg };
    let selected_ink = if row { accent.ink } else { t.selected_ink };
    let primary = if row { accent.color } else { t.form_primary };
    let height = if row { t.space_24 + t.space_16 } else { t.space_24 * 3.0 + t.space_16 };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    ui.painter().rect(
        rect,
        t.radius_lg,
        if selected {
            selected_bg
        } else if response.hovered() {
            t.hover
        } else {
            t.control_fill
        },
        if selected { Stroke::new(if row { 1.0 } else { 2.0 }, primary) } else { t.control_border_stroke(t.control_border) },
        egui::StrokeKind::Inside,
    );
    let radio = pos2(rect.left() + t.space_16, if row { rect.center().y } else { rect.top() + t.space_16 });
    ui.painter().circle(
        radio,
        t.space_6,
        t.control_fill,
        if selected { Stroke::new(1.0, primary) } else { t.control_border_stroke(t.control_border) },
    );
    if selected {
        ui.painter().circle_filled(radio, t.space_3, primary);
    }
    let title = if row {
        Rect::from_min_max(pos2(radio.x + t.space_12, rect.top()), pos2(rect.right() - t.space_8, rect.bottom()))
    } else {
        Rect::from_min_max(pos2(radio.x + t.space_12, rect.top() + t.space_4), pos2(rect.right() - t.space_8, rect.top() + t.space_24 + t.space_4))
    };
    let summary_width = ui.painter().layout_no_wrap(choice.summary.into(), FontId::proportional(t.font_small), t.form_dim).size().x;
    let title = if row { Rect::from_min_max(title.min, title.max - vec2(summary_width + t.space_8, 0.0)) } else { title };
    paint_text(ui, title, choice.title, crate::fonts::semibold(t.font_label), if selected { selected_ink } else { t.form_ink }, false);
    if row {
        let body = Rect::from_min_max(pos2(rect.right() - t.space_8 - summary_width, rect.top()), rect.max - vec2(t.space_8, 0.0));
        paint_text(ui, body, choice.summary, FontId::proportional(t.font_small), t.form_dim, false);
    } else {
        let body = Rect::from_min_max(pos2(rect.left() + t.space_12, title.bottom() + t.space_4), rect.max - vec2(t.space_12, t.space_8));
        let galley = ui.painter().layout(choice.summary.to_owned(), FontId::proportional(t.font_small), t.form_dim, body.width());
        ui.painter().with_clip_rect(ui.clip_rect().intersect(body)).galley(body.min, galley, t.form_dim);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, ui.is_enabled(), selected, format!("{}: {}", choice.title, choice.detail))
    });
    scenario::publish_target(choice.key, rect);
    response.backing_on_hover_text(choice.detail).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A static preview reuses the real agent cursor artwork; it never animates or requests frames.
pub(crate) fn settings_pointer_preview(ui: &mut Ui, visible: bool, speed: &str, follows_camera: bool) {
    let t = Tokens::get();
    let width = dimension(ui.available_width());
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.space_24 * 3.0 + t.space_16), Sense::hover());
    ui.painter().rect(rect, t.radius_lg, t.viewport_top, Stroke::new(1.0, t.dialog_rule), egui::StrokeKind::Inside);
    let path = [pos2(rect.left() + t.space_24, rect.center().y), rect.center(), pos2(rect.right() - t.space_24 * 2.0, rect.center().y - t.space_12)];
    ui.painter().extend(egui::Shape::dashed_line(&path, Stroke::new(1.0, t.control_border), t.space_3, t.space_4));
    for point in path {
        ui.painter().circle_filled(point, t.space_3, t.control_border);
    }
    if visible {
        crate::icons::pointer::paint(&ui.painter().with_clip_rect(rect.shrink(t.space_4)), path.get(1).copied().unwrap_or(rect.center()));
    }
    let caption = if visible {
        crate::i18n::format("ai.prefs.pointer_preview_speed", &[("speed", speed)])
    } else {
        crate::i18n::tr("ai.prefs.pointer_preview_hidden").to_owned()
    };
    let caption_rect = Rect::from_min_max(rect.min + vec2(t.space_8, t.space_4), pos2(rect.right() - t.space_8, rect.top() + t.space_24));
    paint_text(ui, caption_rect, &caption, FontId::proportional(t.font_small), t.form_dim, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), &caption));
    response.backing_on_hover_text(crate::i18n::tr(if follows_camera { "ai.prefs.follow_camera_on" } else { "ai.prefs.follow_camera_off" }));
}

/// One muted metadata line; full commands, endpoints and tool lists remain available on hover.
pub(crate) fn settings_short_note(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_16), Sense::hover());
    paint_text(ui, rect, text, FontId::proportional(t.font_small), t.form_dim, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    response.backing_on_hover_text(text)
}
