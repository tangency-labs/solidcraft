//! Shared concise choice cards and short settings notes.
use super::{metrics::dimension, text::paint_text};
use crate::BackingTooltipExt as _;
use egui::{FontId, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

/// One concise choice with its full meaning available to keyboard users and on hover.
pub struct SettingsChoice<'a> {
    pub key: &'a str,
    pub icon: Option<&'a str>,
    pub title: &'a str,
    pub summary: &'a str,
    pub detail: &'a str,
}

/// Equal option cards keep the choices comparable without repeating long explanations.
pub fn settings_choices(ui: &mut Ui, choices: &[SettingsChoice<'_>], selected: &mut usize) -> bool {
    if super::settings_search::querying(ui) && !super::settings_search::shows_all(ui) {
        let keep: Vec<usize> = choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| super::settings_search::texts_hit(ui, &[choice.title, choice.summary]))
            .map(|(index, _)| index)
            .collect();
        if keep.is_empty() {
            return false;
        }
        super::settings_search::reveal(ui);
        return choice_row(ui, choices, selected, &keep);
    }
    let keep: Vec<usize> = (0..choices.len()).collect();
    choice_row(ui, choices, selected, &keep)
}

fn choice_row(ui: &mut Ui, choices: &[SettingsChoice<'_>], selected: &mut usize, keep: &[usize]) -> bool {
    let t = Tokens::get();
    let count = keep.len().max(1);
    let minimum = keep
        .iter()
        .filter_map(|index| choices.get(*index))
        .map(|choice| super::ContentWidth::name(ui, choice.title) + t.space_24 * 2.0)
        .fold(t.form_label_width * 2.0, f32::max);
    let columns = (((dimension(ui.available_width()) + t.space_8) / (minimum + t.space_8)).floor() as usize).clamp(1, count);
    let width = ((dimension(ui.available_width()) - t.space_8 * columns.saturating_sub(1) as f32) / columns as f32).floor().max(0.0);
    let mut changed = false;
    for row in keep.chunks(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            for index in row {
                let Some(choice) = choices.get(*index) else { continue };
                let response = choice_card(ui, width, choice, *selected == *index, false);
                if response.clicked() {
                    *selected = *index;
                    changed = true;
                }
            }
        });
    }
    changed
}

/// Compact radio tile for preset lists, sharing the settings choice painter and tokens.
pub fn settings_choice_row(ui: &mut Ui, choice: &SettingsChoice<'_>, selected: bool) -> Response {
    choice_card(ui, dimension(ui.available_width()), choice, selected, true)
}
fn choice_card(ui: &mut Ui, width: f32, choice: &SettingsChoice<'_>, selected: bool, row: bool) -> Response {
    let t = Tokens::get();
    let accent = solidcraft_theme::TabAccent::current(ui.ctx());
    let selected_bg = if row { accent.soft } else { t.selected_bg };
    let selected_ink = if row { accent.ink } else { t.selected_ink };
    let primary = if row { accent.color } else { t.form_primary };
    let icon_width = if choice.icon.is_some() { t.space_14 + t.space_6 } else { 0.0 };
    let title_galley = ui.painter().layout(
        choice.title.to_owned(),
        crate::type_face::semibold(t.font_label),
        if selected { selected_ink } else { t.form_ink },
        (width - t.space_24 * 2.0 - icon_width).max(1.0),
    );
    let summary_galley =
        ui.painter().layout(choice.summary.to_owned(), FontId::proportional(t.font_small), t.form_dim, (width - t.space_12 * 2.0).max(1.0));
    let height = if row {
        t.space_24 + t.space_16
    } else {
        (t.space_10 + title_galley.size().y + t.space_6 + summary_galley.size().y + t.space_10).max(t.space_24 * 2.0 + t.space_16 + 1.0)
    };
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
    if row {
        let title = Rect::from_min_max(pos2(radio.x + t.space_12, rect.top()), pos2(rect.right() - t.space_8, rect.bottom()));
        let summary_width = ui.painter().layout_no_wrap(choice.summary.into(), FontId::proportional(t.font_small), t.form_dim).size().x;
        let title = Rect::from_min_max(title.min, title.max - vec2(summary_width + t.space_8, 0.0));
        paint_text(ui, title, choice.title, crate::type_face::semibold(t.font_label), if selected { selected_ink } else { t.form_ink }, false);
        let body = Rect::from_min_max(pos2(rect.right() - t.space_8 - summary_width, rect.top()), rect.max - vec2(t.space_8, 0.0));
        paint_text(ui, body, choice.summary, FontId::proportional(t.font_small), t.form_dim, false);
    } else {
        let mut at = pos2(radio.x + t.space_12, rect.top() + t.space_10);
        if let Some(icon) = choice.icon {
            let cue = Rect::from_min_size(at, vec2(t.space_14, t.space_14));
            crate::icons::paint_icon(ui.painter(), cue, icon, 1.0);
            solidcraft_ui_registry::publish_rect(&format!("{}:icon", choice.key), cue);
            at.x += icon_width;
        }
        let summary_at = pos2(rect.left() + t.space_12, at.y + title_galley.size().y + t.space_6);
        ui.painter().galley(at, title_galley, if selected { selected_ink } else { t.form_ink });
        ui.painter().galley(summary_at, summary_galley, t.form_dim);
    }
    let texts = [choice.title, choice.summary];
    if let Some(text) = texts.iter().find(|text| super::settings_search::texts_hit(ui, &[*text])) {
        super::settings_search::remember(ui, text, rect);
    }
    for text in texts {
        super::settings_search::follow(ui, text, rect);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), selected, format!("{}: {}", choice.title, choice.detail))
    });
    solidcraft_ui_registry::publish_target(choice.key, rect);
    response.backing_on_hover_text(choice.detail).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// One muted metadata line; full commands, endpoints and tool lists remain available on hover.
pub fn settings_short_note(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_16), Sense::hover());
    paint_text(ui, rect, text, FontId::proportional(t.font_small), t.form_dim, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), text));
    response.backing_on_hover_text(text)
}
