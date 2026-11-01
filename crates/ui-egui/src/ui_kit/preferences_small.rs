//! Shared labelled settings blocks and compact explanatory specimens.
use crate::theme::Tokens;
use egui::{Align, FontId, Layout, Rect, Sense, Stroke, Ui, pos2, vec2};

const SETTINGS_PAGE: &str = "sc_settings_page";
pub(super) const ROW_HELP: &str = "sc_settings_row_help";

/// Opt-in page rhythm; other forms retain their existing spacing and heading sizes.
pub(crate) fn settings_page<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag(SETTINGS_PAGE)), |ui| {
        ui.spacing_mut().item_spacing.y = Tokens::get().space_10;
        add(ui)
    })
    .inner
}

pub(super) fn settings_page_active(ui: &Ui) -> bool {
    ui.stack().iter().any(|node| node.tags().contains(SETTINGS_PAGE))
}

/// Indexed standard row with guidance attached to its label, rather than an orphan help row.
pub(crate) fn settings_row_help<R>(ui: &mut Ui, label: &str, help: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag_value(ROW_HELP, help.to_owned())), |ui| {
        super::row(ui, label, add)
    })
    .inner
}

/// A compound form's independently indexed editor, with a short caption above its control.
pub(crate) fn settings_compound_field<R>(ui: &mut Ui, label: &str, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Tokens::get();
    super::settings_form(ui, 0.0, |ui| {
        super::layout::sized_row(ui, label, t.control_height + t.space_16 + t.space_4, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = t.space_4;
                super::settings_inline_note(ui, caption);
                ui.horizontal(add).inner
            })
            .inner
        })
    })
}

#[derive(Clone, Copy)]
pub(crate) enum SettingsBadgeTone {
    Neutral,
    Accent,
    Warning,
    Success,
}

/// A named change indicator; its hover and accessible text explain the dot's meaning.
pub(crate) fn settings_change_dot(ui: &mut Ui, label: &str) -> egui::Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(t.space_8, t.space_8), Sense::hover());
    ui.painter().circle_filled(rect.center(), (t.space_8 - t.space_2 / 2.0) / 2.0, t.pref_note);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), label));
    response.on_hover_text(label)
}

/// One compact card trigger for the pinned profile menu, without a nested input frame.
pub(crate) fn settings_profile_button(ui: &mut Ui, label: &str, name: &str, edited: bool, changed_hint: &str) -> egui::Response {
    let t = Tokens::get();
    let card = super::settings_row_card(ui, false, t.space_24 * 2.0, |ui| {
        let rect = ui.max_rect();
        let side = t.space_16 + t.space_2 / 2.0;
        let icon = Rect::from_center_size(pos2(rect.left() + side / 2.0, rect.center().y), vec2(side, side));
        crate::icons::paint_ink(ui.painter(), icon, "settings_profile", t.form_dim);
        let chevron = Rect::from_center_size(pos2(rect.right() - t.space_14 / 2.0, rect.center().y), vec2(t.space_14, t.space_14));
        super::dropdown_indicator(ui.painter(), chevron, t.form_dim);
        let text_left = (icon.right() + t.space_8).min(chevron.left());
        let text_right = (chevron.left() - t.space_6 - if edited { t.space_8 + t.space_6 } else { 0.0 }).max(text_left);
        let overline = Rect::from_min_max(pos2(text_left, rect.center().y - t.space_16), pos2(text_right, rect.center().y - t.space_2));
        let value = Rect::from_min_max(pos2(text_left, overline.bottom()), pos2(text_right, rect.center().y + t.space_16));
        super::text::paint_text(ui, overline, &label.to_uppercase(), crate::fonts::semibold(t.font_panel), t.form_dim, true);
        super::text::paint_text(ui, value, name, crate::fonts::semibold(t.font_label), t.form_ink, false);
        crate::scenario::publish_rect("prefs:profile:icon", icon);
        crate::scenario::publish_rect("prefs:profile:label", overline);
        crate::scenario::publish_rect("prefs:profile:name", value);
        crate::scenario::publish_rect("prefs:profile:chevron", chevron);
        if edited {
            let dot = Rect::from_min_size(pos2(text_right + t.space_6, value.center().y - t.space_4), vec2(t.space_8, t.space_8));
            let response = ui.scope_builder(egui::UiBuilder::new().max_rect(dot), |ui| settings_change_dot(ui, changed_hint)).inner;
            crate::scenario::publish_target("prefs:profile:edited", response.rect);
        }
    });
    let response = ui.interact(card.response.rect, ui.make_persistent_id("profile_card"), Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    if response.hovered() || response.has_focus() {
        ui.painter().rect_stroke(
            response.rect,
            t.radius_lg,
            Stroke::new(if response.has_focus() { t.space_2 } else { 1.0 }, t.form_primary),
            egui::StrokeKind::Inside,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name));
    crate::scenario::publish_rect("prefs:profile:sidebar_card", response.rect);
    response.on_hover_text(label)
}

/// Compact 18 pt informational badge, with semantic foreground/background pairs.
pub(crate) fn settings_state_badge(ui: &mut Ui, label: &str, tone: SettingsBadgeTone) -> egui::Response {
    let t = Tokens::get();
    let (ink, fill) = match tone {
        SettingsBadgeTone::Neutral => (t.form_dim, t.dialog_footer),
        SettingsBadgeTone::Accent => (t.selected_ink, t.selected_bg),
        SettingsBadgeTone::Warning => (t.pref_note, t.pref_note_fill),
        SettingsBadgeTone::Success => (t.pref_ok, t.pref_ok_fill),
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), crate::fonts::semibold(t.font_small), ink);
    let natural = galley.size().x + t.space_16;
    let width = if settings_page_active(ui) { natural.min(super::metrics::dimension(ui.max_rect().width())) } else { natural };
    let clipped = width < natural;
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.space_16 + t.space_2), Sense::hover());
    ui.painter().rect_filled(rect, t.radius_xl, fill);
    if clipped {
        super::text::paint_text(ui, rect.shrink2(vec2(t.space_8.min(width / 2.0), 0.0)), label, crate::fonts::semibold(t.font_small), ink, false);
    } else {
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), label));
    if clipped { response.on_hover_text(label) } else { response }
}

#[derive(Default)]
pub(crate) struct SettingsChip<'a> {
    pub key: Option<&'a str>,
    pub icon: Option<&'a str>,
    pub accent: bool,
    pub monospace: bool,
    pub remove: Option<&'a str>,
    pub compact: bool,
}

/// Bounded informational pill; an optional removal target lives inside the same outline.
pub(crate) fn settings_chip(ui: &mut Ui, label: &str, chip: SettingsChip<'_>) -> egui::Response {
    let t = Tokens::get();
    let ink = if chip.accent { t.selected_ink } else { t.form_dim };
    let font = if chip.accent {
        crate::fonts::semibold(t.font_caption)
    } else if chip.monospace {
        FontId::monospace(t.font_caption)
    } else {
        FontId::proportional(t.font_caption)
    };
    let icon_side = t.space_12;
    let icon_width = if chip.icon.is_some() { icon_side + t.space_4 } else { 0.0 };
    let remove_width = if chip.remove.is_some() { t.space_24 } else { 0.0 };
    let natural = ui.painter().layout_no_wrap(label.to_owned(), font.clone(), ink).size().x + t.space_16 + icon_width + remove_width;
    let height = if chip.compact { t.space_24 - t.space_2 } else { t.space_24 };
    let (rect, response) = ui.allocate_exact_size(vec2(natural.min(super::metrics::dimension(ui.max_rect().width())), height), Sense::hover());
    ui.painter().rect(
        rect,
        t.radius_xl,
        if chip.accent { t.selected_bg } else { t.control_fill },
        Stroke::new(1.0, if chip.accent { t.form_primary } else { t.dialog_border }),
        egui::StrokeKind::Inside,
    );
    if let Some(icon) = chip.icon {
        let at = Rect::from_center_size(pos2(rect.left() + t.space_8 + icon_side / 2.0, rect.center().y), vec2(icon_side, icon_side));
        if icon == "lock" {
            crate::icons::glyphs::badge(ui.painter(), at, "lock", ink);
        } else {
            crate::icons::paint_ink(ui.painter(), at, icon, ink);
        }
    }
    let text_left = (rect.left() + t.space_8 + icon_width).min(rect.right());
    let text_rect = Rect::from_min_max(pos2(text_left, rect.top()), pos2((rect.right() - t.space_8 - remove_width).max(text_left), rect.bottom()));
    super::text::paint_text(ui, text_rect, label, font, ink, false);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), label));
    if let Some(remove) = chip.remove {
        let target = Rect::from_min_max(pos2((rect.right() - t.space_24).max(rect.left()), rect.top()), rect.max);
        if let Some(key) = chip.key {
            // Allocate the pill in the wrapping parent before giving its action a unique identity.
            let mut action_ui = ui.new_child(egui::UiBuilder::new().id_salt(key).max_rect(target));
            return super::quiet_icon_button_at(&mut action_ui, target, "close", remove).on_hover_text(label);
        }
        return super::quiet_icon_button_at(ui, target, "close", remove).on_hover_text(label);
    }
    response.on_hover_text(label)
}

/// A labelled form block whose control column can contain several natural-height rows.
pub(crate) fn settings_block<R>(ui: &mut Ui, label: &str, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    settings_block_content(ui, label, label_width, add)
}

fn settings_block_content<R>(ui: &mut Ui, label: &str, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    if let Some(gate) = crate::prefs_search::block_gate(ui, label, None) {
        let before = crate::prefs_search::hits(ui);
        if !gate.show {
            let result = crate::prefs_search::run_hidden(ui, label, add);
            crate::prefs_search::block_finish(ui, &gate, false, before);
            return result;
        }
        let result = settings_block_body(ui, label, label_width, add);
        crate::prefs_search::block_finish(ui, &gate, true, before);
        return result;
    }
    settings_block_body(ui, label, label_width, add)
}

fn settings_block_body<R>(ui: &mut Ui, label: &str, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
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
    if settings_page_active(ui) {
        crate::scenario::publish_rect("prefs:autosave_timeline", r);
    }
    ui.painter().hline(line.x_range(), r.top() + t.space_10, Stroke::new(2.0, t.dialog_rule));
    if minutes.is_finite() && minutes > 0.0 {
        let ticks = (30.0 / minutes).floor().clamp(0.0, 60.0) as u32;
        for i in 0..=ticks {
            let x = line.right() - line.width() * (f64::from(i) * minutes / 30.0) as f32;
            ui.painter().vline(x, r.top() + t.space_4..=r.top() + t.space_16, Stroke::new(2.0, t.form_primary));
        }
    }
    for (share, key) in [(0.0, "prefs.timeline_half_hour"), (0.5, "prefs.timeline_quarter_hour"), (1.0, "prefs.timeline_now")] {
        if settings_page_active(ui) && share == 0.5 {
            continue;
        }
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
