//! Shared labelled settings blocks and compact explanatory specimens.
use crate::BackingTooltipExt as _;
use egui::{Align, FontId, Layout, Rect, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

const SETTINGS_PAGE: &str = "sc_settings_page";
pub(super) const ROW_HELP: &str = "sc_settings_row_help";

/// Opt-in page rhythm; other forms retain their existing spacing and heading sizes.
pub fn settings_page<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag(SETTINGS_PAGE)), |ui| {
        ui.spacing_mut().item_spacing.y = Tokens::get().space_10;
        add(ui)
    })
    .inner
}

pub fn settings_page_active(ui: &Ui) -> bool {
    ui.stack().iter().any(|node| node.tags().contains(SETTINGS_PAGE))
}

/// Indexed standard row with guidance attached to its label, rather than an orphan help row.
pub fn settings_row_help<R>(ui: &mut Ui, label: &str, help: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag_value(ROW_HELP, help.to_owned())), |ui| {
        super::row(ui, label, add)
    })
    .inner
}

/// A compound form's independently indexed editor, with a short caption above its control.
pub fn settings_compound_field<R>(ui: &mut Ui, label: &str, caption: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
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
pub enum SettingsBadgeTone {
    Neutral,
    Accent,
    Warning,
    Success,
}

/// A named change indicator; its hover and accessible text explain the dot's meaning.
pub fn settings_change_dot(ui: &mut Ui, label: &str) -> egui::Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(t.space_8, t.space_8), Sense::hover());
    ui.painter().circle_filled(rect.center(), (t.space_8 - t.space_2 / 2.0) / 2.0, t.pref_note);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), label));
    response.backing_on_hover_text(label)
}

/// One compact card trigger for the pinned profile menu, without a nested input frame.
pub fn settings_profile_button(ui: &mut Ui, label: &str, name: &str, edited: bool, changed_hint: &str) -> egui::Response {
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
        super::text::paint_text(ui, overline, &label.to_uppercase(), crate::type_face::semibold(t.font_panel), t.form_dim, true);
        super::text::paint_text(ui, value, name, crate::type_face::semibold(t.font_label), t.form_ink, false);
        solidcraft_ui_registry::publish_rect("prefs:profile:icon", icon);
        solidcraft_ui_registry::publish_rect("prefs:profile:label", overline);
        solidcraft_ui_registry::publish_rect("prefs:profile:name", value);
        solidcraft_ui_registry::publish_rect("prefs:profile:chevron", chevron);
        if edited {
            let dot = Rect::from_min_size(pos2(text_right + t.space_6, value.center().y - t.space_4), vec2(t.space_8, t.space_8));
            let response = ui.scope_builder(egui::UiBuilder::new().max_rect(dot), |ui| settings_change_dot(ui, changed_hint)).inner;
            solidcraft_ui_registry::publish_target("prefs:profile:edited", response.rect);
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
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), name));
    solidcraft_ui_registry::publish_rect("prefs:profile:sidebar_card", response.rect);
    response.backing_on_hover_text(label)
}

/// Compact 18 pt informational badge, with semantic foreground/background pairs.
pub fn settings_state_badge(ui: &mut Ui, label: &str, tone: SettingsBadgeTone) -> egui::Response {
    let t = Tokens::get();
    let (ink, fill) = match tone {
        SettingsBadgeTone::Neutral => (t.form_dim, t.dialog_footer),
        SettingsBadgeTone::Accent => (t.selected_ink, t.selected_bg),
        SettingsBadgeTone::Warning => (t.pref_note, t.pref_note_fill),
        SettingsBadgeTone::Success => (t.pref_ok, t.pref_ok_fill),
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), crate::type_face::semibold(t.font_small), ink);
    let natural = galley.size().x + t.space_16;
    let width = if settings_page_active(ui) { natural.min(super::metrics::dimension(ui.max_rect().width())) } else { natural };
    let clipped = width < natural;
    let (rect, response) = ui.allocate_exact_size(vec2(width, t.space_16 + t.space_2), Sense::hover());
    ui.painter().rect_filled(rect, t.radius_xl, fill);
    if clipped {
        super::text::paint_text(ui, rect.shrink2(vec2(t.space_8.min(width / 2.0), 0.0)), label, crate::type_face::semibold(t.font_small), ink, false);
    } else {
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), label));
    if clipped { response.backing_on_hover_text(label) } else { response }
}

#[derive(Default)]
pub struct SettingsChip<'a> {
    pub key: Option<&'a str>,
    pub icon: Option<&'a str>,
    pub accent: bool,
    pub monospace: bool,
    pub remove: Option<&'a str>,
    pub compact: bool,
}

/// Bounded informational pill; an optional removal target lives inside the same outline.
pub fn settings_chip(ui: &mut Ui, label: &str, chip: SettingsChip<'_>) -> egui::Response {
    super::settings_search::note_unlabelled(ui, &[label]);
    if super::settings_search::querying(ui) && !super::settings_search::visible(ui, &[label]) {
        return super::settings_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let ink = if chip.accent { t.selected_ink } else { t.form_dim };
    let font = if chip.accent {
        crate::type_face::semibold(t.font_caption)
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
    if super::layout::label(ui).is_empty() {
        super::settings_search::remember(ui, label, rect);
        super::settings_search::follow(ui, label, rect);
    }
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
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), label));
    if let Some(remove) = chip.remove {
        let target = Rect::from_min_max(pos2((rect.right() - t.space_24).max(rect.left()), rect.top()), rect.max);
        if let Some(key) = chip.key {
            // Allocate the pill in the wrapping parent before giving its action a unique identity.
            let mut action_ui = ui.new_child(egui::UiBuilder::new().id_salt(key).max_rect(target));
            return super::quiet_icon_button_at(&mut action_ui, target, "close", remove).backing_on_hover_text(label);
        }
        return super::quiet_icon_button_at(ui, target, "close", remove).backing_on_hover_text(label);
    }
    response.backing_on_hover_text(label)
}

/// A labelled form block whose control column can contain several natural-height rows.
pub fn settings_block<R>(ui: &mut Ui, label: &str, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    settings_block_hint(ui, label, None, label_width, add)
}

/// A form block whose explanatory hint belongs below its label, beside the controls.
pub fn settings_block_hint<'a, R>(ui: &mut Ui, label: &str, hint: impl Into<Option<&'a str>>, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    settings_block_content(ui, label, hint.into(), label_width, add)
}

pub fn settings_block_content<R>(ui: &mut Ui, label: &str, hint: Option<&str>, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    if let Some(gate) = super::settings_search::block_gate(ui, label, hint) {
        let before = super::settings_search::hits(ui);
        if !gate.show {
            let result = super::settings_search::run_hidden(ui, label, add);
            super::settings_search::block_finish(ui, &gate, false, before);
            return result;
        }
        // A block that matches is a result even when its control reports none (a plain number
        // field, as Sketch › Window steps has): without this the page and section headings never
        // appear and the window says nothing matched.
        if gate.matched {
            super::settings_search::reveal(ui);
        }
        let stops = super::settings_search::nav_len(ui);
        let (result, rect) = settings_block_body(ui, label, hint, label_width, add);
        // The arrow keys reach it too, unless a control inside already gave it a stop.
        // A block found only by its hint is a stop too; its label is what Enter opens.
        if super::settings_search::nav_len(ui) == stops {
            super::settings_search::remember_matched(ui, label, hint, rect);
        }
        super::settings_search::block_finish(ui, &gate, true, before);
        return result;
    }
    let (result, rect) = settings_block_body(ui, label, hint, label_width, add);
    // After Enter opened this block's page from a search, scroll to it and outline it.
    super::settings_search::follow(ui, label, rect);
    result
}

pub fn settings_block_body<R>(ui: &mut Ui, label: &str, hint: Option<&str>, label_width: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, Rect) {
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
                // Two blocks with the same internal layout would otherwise share their controls'
                // ids, so a later block's segmented control could stop an earlier one from taking
                // clicks (#14121).
                .id_salt(("settings_block", label))
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
    solidcraft_ui_registry::publish_rect(&format!("dialog:row:{label}"), response.response.rect);
    (response.inner, response.response.rect)
}

/// A bounded control that keeps its intended width inside a larger form column.
pub fn settings_control_width<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let width = super::metrics::dimension(width).min(super::metrics::dimension(ui.available_width()));
    ui.allocate_ui_with_layout(vec2(width, Tokens::get().control_height), Layout::left_to_right(Align::Center), add).inner
}

/// A small numeric control, using the same number editor and geometry as every form.
pub fn settings_number<T: egui::emath::Numeric>(ui: &mut Ui, value: &mut T, range: std::ops::RangeInclusive<T>, speed: f64) -> egui::Response {
    ui.scope(|ui| {
        let t = Tokens::get();
        ui.spacing_mut().interact_size.x = t.form_label_width - t.space_6;
        super::number_field(ui, value, range, speed, "")
    })
    .inner
}

/// [`settings_number`] for a whole number, with step arrows (`whole_number_field`, #11829).
pub fn settings_whole_number<T: egui::emath::Numeric>(ui: &mut Ui, value: &mut T, range: std::ops::RangeInclusive<T>) -> egui::Response {
    ui.scope(|ui| {
        let t = Tokens::get();
        ui.spacing_mut().interact_size.x = t.form_label_width - t.space_6;
        super::whole_number_field(ui, value, range, "")
    })
    .inner
}

/// A keyboard binding table: keycaps are separate from actions and never imply fixed bindings.
pub fn settings_bindings(ui: &mut Ui, bindings: &[(&[&str], &str)]) {
    let t = Tokens::get();
    let font = FontId::proportional(t.font_small);
    let key_width = bindings
        .iter()
        .map(|(keys, _)| {
            keys.iter()
                .map(|key| ui.painter().layout_no_wrap((*key).to_owned(), font.clone(), t.form_ink).size().x.ceil() + t.space_12 + t.space_4)
                .sum::<f32>()
        })
        .fold(t.space_24 * 8.0 + t.space_8, f32::max);
    for (keys, action) in bindings {
        if super::settings_search::querying(ui) && !super::settings_search::shows_all(ui) {
            if !super::settings_search::texts_hit(ui, &[action, &keys.join(" ")]) {
                continue;
            }
            super::settings_search::reveal(ui);
        }
        let width = super::metrics::dimension(ui.available_width());
        let action_width = super::ContentWidth::label(ui, action);
        let beside = key_width + t.space_10 + action_width <= width;
        let limit = if beside { key_width } else { width };
        let cap_height = t.control_height - t.space_8;
        let mut caps = Vec::new();
        let (mut x, mut y) = (0.0, t.space_4);
        for key in *keys {
            let galley = ui.painter().layout_no_wrap((*key).to_owned(), font.clone(), t.form_ink);
            let cap_width = (galley.size().x.ceil() + t.space_12).min(limit);
            if x > 0.0 && x + cap_width > limit {
                x = 0.0;
                y += cap_height + t.space_4;
            }
            caps.push((*key, Rect::from_min_size(pos2(x, y), vec2(cap_width, cap_height))));
            x += cap_width + t.space_4;
        }
        let keys_bottom = y + cap_height + t.space_4;
        let action_left = if beside { key_width + t.space_10 } else { 0.0 };
        let action_top = if beside { 0.0 } else { keys_bottom };
        let action = ui.painter().layout((*action).to_owned(), FontId::proportional(t.font_label), t.form_ink, (width - action_left).max(0.0));
        let height = (action_top + action.size().y + t.space_4).max(keys_bottom).max(t.control_height + t.space_2);
        let (r, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), format!("{}: {}", keys.join(" + "), action.text())));
        t.secondary_hline(ui.painter(), r.x_range(), r.bottom(), t.dialog_rule);
        for (key, cap) in caps {
            let cap = cap.translate(r.min.to_vec2());
            ui.painter().rect(cap, t.radius_sm, t.control_fill, t.container_border(t.control_border), egui::StrokeKind::Inside);
            super::text::paint_text(ui, cap.shrink2(vec2(t.space_6.min(cap.width() / 2.0), 0.0)), key, font.clone(), t.form_ink, false);
        }
        let action_y = if beside { r.center().y - action.size().y / 2.0 } else { r.top() + action_top };
        ui.painter().with_clip_rect(r.intersect(ui.clip_rect())).galley(pos2(r.left() + action_left, action_y), action, t.form_ink);
    }
}
/// Natural-width, dim monospaced numeric specimen beside a precision control.
pub fn settings_precision_sample(ui: &mut Ui, text: &str) {
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::monospace(t.font_body), t.form_dim);
    let (r, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), text));
    ui.painter().galley(r.min, galley, t.form_dim);
}
