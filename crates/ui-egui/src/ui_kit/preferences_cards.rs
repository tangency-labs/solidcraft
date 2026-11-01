//! The controls the redesigned Preferences pages share: picture cards, toggle tiles, preset cards,
//! the hover-help icon and the carousel. Sizes follow `docs/design/preferences/measurements.md`;
//! every colour comes from `Tokens`. A card sizes to the width its container gives it and its
//! figure keeps the figure's aspect ratio.
use super::{metrics::dimension, preferences::centred_text};
use crate::{scenario, theme::Tokens};
use egui::{Color32, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

/// How a card or tile looks in a state. Kept separate from painting so the state rules can be tested.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CardLook {
    /// Border width in points: 2 when selected, else 1.
    pub border: f32,
    pub fill: CardFill,
    pub ink: CardInk,
    pub bold: bool,
    /// Multiplier on every colour; a disabled card is dimmed.
    pub alpha: f32,
    /// A keyboard focus ring outside the border.
    pub focus_ring: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CardFill {
    Face,
    Hover,
    Selected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CardInk {
    Text,
    Accent,
}

pub(super) fn card_look(selected: bool, enabled: bool, hovered: bool, focused: bool) -> CardLook {
    CardLook {
        border: if selected { 2.0 } else { 1.0 },
        fill: if selected {
            CardFill::Selected
        } else if enabled && hovered {
            CardFill::Hover
        } else {
            CardFill::Face
        },
        ink: if selected { CardInk::Accent } else { CardInk::Text },
        bold: selected,
        alpha: if enabled { 1.0 } else { 0.5 },
        focus_ring: enabled && focused,
    }
}

/// What a picture card or toggle tile shows.
pub(crate) struct Card<'a> {
    pub label: &'a str,
    /// One short line under the label ("default", "Built in").
    pub sub: Option<&'a str>,
    /// Selected (radio) or on (tile).
    pub selected: bool,
    pub enabled: bool,
    /// The small "has options" disc at the top right.
    pub options: Option<&'a str>,
    /// An accent check at the top right (picture cards only).
    pub check: bool,
    /// Width over height of the figure.
    pub aspect: f32,
    pub figure: &'a dyn Fn(&Painter, Rect),
}

/// The width of one of `columns` cards in `available` points with `gap` between them.
pub(super) fn card_width(available: f32, columns: usize, gap: f32) -> f32 {
    let columns = columns.max(1) as f32;
    let available = dimension(available);
    ((available - gap.max(0.0) * (columns - 1.0)) / columns).max(0.0)
}

fn card_height(t: &Tokens, width: f32, card: &Card) -> f32 {
    let pad = t.space_6;
    let aspect = if card.aspect.is_finite() && card.aspect > 0.01 { card.aspect } else { 1.0 };
    let figure = ((width - 2.0 * pad).max(0.0) / aspect).min(dimension(f32::MAX));
    let sub = if card.sub.is_some() { t.space_14 } else { 0.0 };
    pad + figure + gap(t) + t.space_16 + sub + pad + 1.0
}

fn gap(t: &Tokens) -> f32 {
    (t.space_4 + t.space_6) * 0.5
}

/// A bounded settings row/card using the same selection treatment as picture cards.
pub(crate) fn settings_row_card<R>(ui: &mut Ui, selected: bool, height: f32, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), height), Sense::hover());
    let look = card_look(selected, ui.is_enabled(), false, false);
    ui.painter().rect(
        rect,
        t.radius_lg,
        if selected { t.selected_bg } else { t.control_fill },
        Stroke::new(look.border, if selected { t.form_primary } else { t.dialog_border }),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(t.space_10, 0.0));
    let result = ui
        .scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(inner));
            ui.set_min_size(inner.size());
            add(ui)
        })
        .inner;
    egui::InnerResponse { inner: result, response }
}

fn paint_card(ui: &mut Ui, card: &Card, width: f32, tile: bool) -> Response {
    crate::prefs_search::note_unlabelled(ui, &[card.label]);
    if crate::prefs_search::querying(ui) && !crate::prefs_search::visible(ui, &[card.label, card.sub.unwrap_or("")]) {
        return crate::prefs_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let width = dimension(width);
    let enabled = card.enabled && ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(vec2(width, card_height(&t, width, card)), if enabled { Sense::click() } else { Sense::hover() });
    // Labelled compound rows own their search index (settings_picture_choices, for example).
    // Standalone cards and tiles are independently searchable settings.
    if super::layout::label(ui).is_empty() {
        crate::prefs_search::remember(ui, card.label, rect);
        crate::prefs_search::follow(ui, card.label, rect);
    }
    let look = card_look(card.selected, enabled, response.hovered(), response.has_focus());
    let p = ui.painter();
    let dim = |c: Color32| c.gamma_multiply(look.alpha);
    let fill = match look.fill {
        CardFill::Selected => t.selected_bg,
        CardFill::Hover => t.hover,
        CardFill::Face => t.control_fill,
    };
    let border = if card.selected { t.form_primary } else { t.dialog_border };
    let radius = f32::from(t.radius_lg);
    p.rect(rect, radius, dim(fill), Stroke::new(look.border, dim(border)), StrokeKind::Inside);
    if look.focus_ring {
        p.rect_stroke(rect.expand(t.space_2), radius + t.space_2, Stroke::new(t.space_2, t.form_primary), StrokeKind::Outside);
    }
    let inset = t.space_6;
    let figure = Rect::from_min_size(
        rect.min + vec2(inset, inset),
        vec2((rect.width() - 2.0 * inset).max(0.0), ((rect.width() - 2.0 * inset).max(0.0) / card.aspect.max(0.01)).min(rect.height())),
    );
    if figure.is_positive() {
        let figure_painter = p.with_clip_rect(figure.intersect(p.clip_rect()));
        (card.figure)(&figure_painter, figure);
        if !enabled {
            p.rect_filled(figure, 0.0, t.control_fill.gamma_multiply(0.5));
        }
    }
    let ink = match look.ink {
        CardInk::Accent => t.selected_ink,
        CardInk::Text => t.form_ink,
    };
    let font = if look.bold { crate::fonts::semibold(t.font_label) } else { FontId::proportional(t.font_label) };
    let label_rect =
        Rect::from_min_size(pos2(rect.left() + t.space_2, figure.bottom() + gap(&t)), vec2((rect.width() - 2.0 * t.space_2).max(0.0), t.space_16));
    centred_text(p, label_rect, card.label, font, dim(ink));
    if let Some(sub) = card.sub {
        let sub_rect = Rect::from_min_size(pos2(label_rect.left(), label_rect.bottom()), vec2(label_rect.width(), t.space_14));
        centred_text(p, sub_rect, sub, FontId::proportional(t.font_small), dim(t.form_dim));
    }
    let corner = pos2(rect.right() - t.space_6, rect.top() + t.space_6);
    if tile {
        // The checkbox in the corner: 15 points, radius 4, accent with a tick when on.
        let size = t.space_14 + 1.0;
        let box_rect = Rect::from_min_size(corner - vec2(size, 0.0), vec2(size, size));
        if card.selected {
            p.rect_filled(box_rect, t.space_4, dim(t.form_primary));
            tick(p, box_rect, dim(t.on_accent));
        } else {
            p.rect(box_rect, t.space_4, dim(t.control_fill), Stroke::new(1.0, dim(t.control_border)), StrokeKind::Inside);
        }
    } else if card.options.is_some() || card.check {
        let size = t.space_16;
        let centre = pos2(rect.right() - t.space_6 + 1.0 - size * 0.5, rect.top() + t.space_6 - 1.0 + size * 0.5);
        p.circle_filled(centre, size * 0.5, dim(t.form_primary));
        if card.check {
            tick(p, Rect::from_center_size(centre, vec2(size, size) * 0.75), dim(t.on_accent));
        } else {
            let dot = Rect::from_center_size(centre, vec2(size, size) * 0.6);
            crate::icons::paint_ink(p, dot, "settings", dim(t.on_accent));
        }
    }
    let kind = if tile { egui::WidgetType::Checkbox } else { egui::WidgetType::RadioButton };
    response.widget_info(|| egui::WidgetInfo::selected(kind, enabled, card.selected, card.label));
    scenario::publish_target(&format!("prefs:card:{}", card.label), rect);
    let response = response.on_hover_text(card.label);
    let response = if let Some(options) = card.options { response.on_hover_text(options) } else { response };
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

fn tick(p: &Painter, r: Rect, ink: Color32) {
    let w = (r.width() * 0.16).max(1.0);
    let a = pos2(r.left() + r.width() * 0.24, r.top() + r.height() * 0.52);
    let b = pos2(r.left() + r.width() * 0.42, r.top() + r.height() * 0.70);
    let c = pos2(r.left() + r.width() * 0.78, r.top() + r.height() * 0.32);
    p.line_segment([a, b], Stroke::new(w, ink));
    p.line_segment([b, c], Stroke::new(w, ink));
}

/// A radio card: a figure, a label and an optional sub-label. Click it to choose it.
pub(crate) fn picture_card(ui: &mut Ui, card: &Card, width: f32) -> Response {
    paint_card(ui, card, width, false)
}

/// A real radio choice backed by the canonical preference figure painter.
pub(crate) struct PictureChoice<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub figure: fn(&Painter, Rect),
}

impl PictureChoice<'_> {
    fn card(&self, selected: bool, enabled: bool, aspect: f32) -> Card<'_> {
        Card { label: self.label, sub: None, selected, enabled, options: None, check: selected, aspect, figure: &self.figure }
    }
}

/// Indexed, wrapping picture choices. The row and every option point at the shipped cards.
pub(crate) fn settings_picture_choices(
    ui: &mut Ui,
    key: &str,
    label: &str,
    options: &[PictureChoice<'_>],
    selected: usize,
    enabled: bool,
) -> Option<usize> {
    let t = Tokens::get();
    settings_picture_choices_sized(
        ui,
        key,
        label,
        options,
        selected,
        enabled,
        PictureChoiceSize { width: t.space_24 * 4.0 + t.space_16 + t.space_4, aspect: 226.0 / 148.0, caption: true },
    )
}

/// Page-specific card proportions; the ordinary History/Browser composition keeps its defaults.
pub(crate) struct PictureChoiceSize {
    pub width: f32,
    pub aspect: f32,
    /// Omit a repeated caption when the enclosing section already names the setting.
    pub caption: bool,
}

pub(crate) fn settings_picture_choices_sized(
    ui: &mut Ui,
    key: &str,
    label: &str,
    options: &[PictureChoice<'_>],
    selected: usize,
    enabled: bool,
    size: PictureChoiceSize,
) -> Option<usize> {
    let t = Tokens::get();
    let available = dimension(ui.available_width());
    let width = options.iter().map(|o| super::ContentWidth::name(ui, o.label) + t.space_12).fold(size.width, f32::max).min(available);
    let ppp = ui.ctx().pixels_per_point().max(0.5);
    let width = (width * ppp).floor() / ppp;
    let columns = (((available + t.space_10) / (width + t.space_10).max(1.0)).floor() as usize).max(1);
    let rows = options.len().div_ceil(columns);
    let height = options.first().map_or(0.0, |o| card_height(&t, width, &o.card(selected == 0, enabled, size.aspect))) * rows as f32
        + rows.saturating_sub(1) as f32 * t.space_10
        + if size.caption { t.space_16 + t.space_10 } else { 0.0 };
    super::settings_form(ui, 0.0, |ui| {
        super::layout::sized_row(ui, label, height, |ui| {
            crate::prefs_search::note_current(ui, &options.iter().map(|o| o.label).collect::<Vec<_>>().join(" "));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = vec2(t.space_10, t.space_10);
                // Use the real control child, including the scrollbar and form bounds. Round
                // down so the final card border fits together with every intervening gap.
                let available = dimension(ui.available_width());
                let width = (width.min(available) * ppp).floor() / ppp;
                let columns = (((available + t.space_10) / (width + t.space_10).max(1.0)).floor() as usize).max(1);
                if size.caption {
                    super::settings_inline_note(ui, label);
                }
                let mut picked = None;
                let mut indexed = crate::prefs_search::texts_hit(ui, &[label]);
                for (row, choices) in options.chunks(columns).enumerate() {
                    ui.horizontal_top(|ui| {
                        for (column, option) in choices.iter().enumerate() {
                            let index = row * columns + column;
                            let response = picture_card(ui, &option.card(selected == index, enabled, size.aspect), width);
                            scenario::publish_target(&format!("prefs:{key}:{}", option.key), response.rect);
                            scenario::publish_target(&format!("dialog:{label}:{}", option.label), response.rect);
                            if !indexed && crate::prefs_search::texts_hit(ui, &[option.label]) {
                                crate::prefs_search::remember(ui, option.label, response.rect);
                                indexed = true;
                            }
                            crate::prefs_search::follow(ui, option.label, response.rect);
                            if response.clicked() && enabled {
                                picked = Some(index);
                            }
                        }
                    });
                }
                picked
            })
            .inner
        })
    })
}

/// A toggle tile: the same picture with a checkbox in the corner. Click it to flip `card.selected`.
pub(crate) fn toggle_tile(ui: &mut Ui, card: &Card, width: f32) -> Response {
    paint_card(ui, card, width, true)
}

/// A preset card: an icon, a name, one word and a check. `icon` paints into the 38-point box in
/// the colour it is given.
pub(crate) fn preset_card(
    ui: &mut Ui,
    name: &str,
    word: &str,
    selected: bool,
    enabled: bool,
    icon: &dyn Fn(&Painter, Rect, Color32),
    width: f32,
) -> Response {
    crate::prefs_search::note_unlabelled(ui, &[name, word]);
    if crate::prefs_search::querying(ui) && !crate::prefs_search::visible(ui, &[name, word]) {
        return crate::prefs_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let enabled = enabled && ui.is_enabled();
    let width = dimension(width);
    let height = t.space_24 * 3.0 - t.space_2;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), if enabled { Sense::click() } else { Sense::hover() });
    crate::prefs_search::remember(ui, name, rect);
    crate::prefs_search::follow(ui, name, rect);
    let look = card_look(selected, enabled, response.hovered(), response.has_focus());
    let dim = |c: Color32| c.gamma_multiply(look.alpha);
    let p = ui.painter();
    let fill = match look.fill {
        CardFill::Selected => t.selected_bg,
        CardFill::Hover => t.hover,
        CardFill::Face => t.control_fill,
    };
    let radius = f32::from(t.radius_lg) + t.space_2;
    p.rect(rect, radius, dim(fill), Stroke::new(look.border, dim(if selected { t.form_primary } else { t.dialog_border })), StrokeKind::Inside);
    if look.focus_ring {
        p.rect_stroke(rect.expand(t.space_2), radius + t.space_2, Stroke::new(t.space_2, t.form_primary), StrokeKind::Outside);
    }
    let box_size = t.space_24 + t.space_14;
    let box_rect = Rect::from_min_size(pos2(rect.left() + t.space_14, rect.center().y - box_size * 0.5), vec2(box_size, box_size));
    let box_fill = if selected { t.form_primary } else { t.pref_well };
    let box_ink = if selected { t.on_accent } else { t.form_dim };
    p.rect_filled(box_rect, f32::from(t.radius_lg) + t.space_2, dim(box_fill));
    icon(p, Rect::from_center_size(box_rect.center(), box_rect.size() * 0.55), dim(box_ink));
    let text_left = box_rect.right() + t.space_12;
    let text_width = (rect.right() - t.space_14 - t.space_24 - text_left).max(0.0);
    let name_font = crate::fonts::semibold(t.font_heading);
    let name_ink = if selected { t.selected_ink } else { t.form_ink };
    let name_rect = Rect::from_min_size(pos2(text_left, rect.center().y - t.space_16), vec2(text_width, t.space_16));
    super::text::paint_text(ui, name_rect, name, name_font, dim(name_ink), false);
    let word_rect = Rect::from_min_size(pos2(text_left, name_rect.bottom()), vec2(text_width, t.space_16));
    super::text::paint_text(ui, word_rect, word, FontId::proportional(t.font_small), dim(t.form_dim), false);
    let p = ui.painter();
    if selected {
        let centre = pos2(rect.right() - t.space_14 - t.space_8 + 1.0, rect.center().y);
        p.circle_filled(centre, t.space_8 + 1.0, dim(t.form_primary));
        tick(p, Rect::from_center_size(centre, vec2(t.space_12, t.space_12)), dim(t.on_accent));
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, selected, name));
    scenario::publish_target(&format!("prefs:preset:{name}"), rect);
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

/// The (i) icon after a label: shows `text` as a tooltip and tells assistive technology the same.
pub(crate) fn help_icon(ui: &mut Ui, text: &str) -> Response {
    let t = Tokens::get();
    let size = t.space_12 + 1.0;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let ink = if response.hovered() { t.form_ink } else { t.form_dim };
    let p = ui.painter();
    p.circle_stroke(rect.center(), size * 0.5 - 0.5, Stroke::new(1.0, ink));
    p.circle_filled(pos2(rect.center().x, rect.top() + size * 0.3), 0.9, ink);
    p.line_segment([pos2(rect.center().x, rect.top() + size * 0.46), pos2(rect.center().x, rect.bottom() - size * 0.24)], Stroke::new(1.2, ink));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    scenario::publish_target("prefs:help_icon", rect);
    response.on_hover_text(text)
}

/// Which items of a carousel are shown: the first one and how many fit between the two arrows.
pub(super) fn carousel_window(count: usize, offset: usize, available: f32, item_width: f32, gap: f32, arrow: f32) -> (usize, usize) {
    if count == 0 {
        return (0, 0);
    }
    let room = dimension(available) - 2.0 * (arrow + gap);
    let unit = dimension(item_width).max(1.0) + gap.max(0.0);
    let visible = (((room + gap) / unit).floor() as usize).clamp(1, count);
    (offset.min(count - visible), visible)
}

/// A row of items with a previous and a next arrow; an arrow is dimmed and inert at its end.
/// `item` paints the item at an index into a column `item_width` wide and `item_height` high.
pub(crate) fn carousel(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    count: usize,
    item_width: f32,
    item_height: f32,
    mut item: impl FnMut(&mut Ui, usize),
) -> (usize, usize) {
    let t = Tokens::get();
    let key = ui.make_persistent_id(id);
    let gap = t.space_8;
    let arrow = t.space_24 + 2.0;
    let offset = ui.data(|d| d.get_temp::<usize>(key)).unwrap_or(0);
    let (first, visible) = carousel_window(count, offset, ui.available_width(), item_width, gap, arrow);
    let height = dimension(item_height);
    let mut next_offset = first;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        let can_back = first > 0;
        if carousel_arrow(ui, vec2(arrow, height), -1.0, can_back, crate::i18n::tr("prefs.carousel_previous")).clicked() {
            next_offset = first.saturating_sub(1);
        }
        for index in first..first + visible {
            ui.allocate_ui_with_layout(vec2(dimension(item_width), height), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(dimension(item_width));
                item(ui, index);
            });
        }
        let can_forward = first + visible < count;
        if carousel_arrow(ui, vec2(arrow, height), 1.0, can_forward, crate::i18n::tr("prefs.carousel_next")).clicked() {
            next_offset = (first + 1).min(count.saturating_sub(visible));
        }
    });
    ui.data_mut(|d| d.insert_temp(key, next_offset));
    (first, visible)
}

fn carousel_arrow(ui: &mut Ui, size: egui::Vec2, direction: f32, enabled: bool, label: &str) -> Response {
    let t = Tokens::get();
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let alpha = if enabled { 1.0 } else { 0.35 };
    let p = ui.painter();
    let fill = if enabled && response.hovered() { t.hover } else { t.control_fill };
    p.rect(rect, f32::from(t.radius_lg), fill.gamma_multiply(alpha), Stroke::new(1.0, t.dialog_border.gamma_multiply(alpha)), StrokeKind::Inside);
    let icon = Rect::from_center_size(rect.center(), vec2(t.space_16, t.space_16));
    let ink = t.form_ink.gamma_multiply(alpha);
    if direction < 0.0 {
        crate::icons::paint_ink_mirrored(p, icon, "chevron_right", ink);
    } else {
        crate::icons::paint_ink(p, icon, "chevron_right", ink);
    }
    if enabled && response.has_focus() {
        p.rect_stroke(rect.expand(t.space_2), f32::from(t.radius_lg) + t.space_2, Stroke::new(t.space_2, t.form_primary), StrokeKind::Outside);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    scenario::publish_target(&format!("prefs:carousel:{label}"), rect);
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}
