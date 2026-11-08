//! Shared theme browser vocabulary: toggle segments, slots, marked cards and scroll viewport.
use super::*;
use crate::theme::Tokens;
use egui::{Align2, Color32, FontId, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ThemeGlyph {
    None,
    Sun,
    Moon,
    Contrast,
    Monitor,
    Carousel,
    Grid,
    Check,
    Import,
    Previous,
    Next,
}

fn glyph(p: &Painter, r: Rect, kind: ThemeGlyph, ink: Color32) {
    if !r.is_finite() || !r.is_positive() {
        return;
    }
    let scale = r.width().min(r.height()) / 24.0;
    let origin = r.center() - egui::Vec2::splat(Tokens::get().space_12) * scale;
    let at = |x, y| origin + vec2(x, y) * scale;
    let line = |points: &[[f32; 2]], width| {
        p.add(egui::Shape::line(points.iter().map(|[x, y]| at(*x, *y)).collect(), Stroke::new(width * scale, ink)));
    };
    let stroke = Stroke::new(2.0 * scale, ink);
    match kind {
        ThemeGlyph::None => {}
        ThemeGlyph::Sun => {
            p.circle_stroke(at(12.0, 12.0), 4.0 * scale, stroke);
            for points in [
                [[12.0, 2.0], [12.0, 5.0]],
                [[12.0, 19.0], [12.0, 22.0]],
                [[2.0, 12.0], [5.0, 12.0]],
                [[19.0, 12.0], [22.0, 12.0]],
                [[5.0, 5.0], [7.0, 7.0]],
                [[17.0, 17.0], [19.0, 19.0]],
                [[19.0, 5.0], [17.0, 7.0]],
                [[7.0, 17.0], [5.0, 19.0]],
            ] {
                line(&points, 2.0);
            }
        }
        ThemeGlyph::Moon => {
            // SVG A8 8 0 1 1 10 4: the large clockwise arc from (20,14).
            let centre = vec2(15.0 - 7.0_f32.sqrt(), 9.0 + 7.0_f32.sqrt());
            let start = (14.0 - centre.y).atan2(20.0 - centre.x);
            let end = (4.0 - centre.y).atan2(10.0 - centre.x) + std::f32::consts::TAU;
            let points: Vec<_> = (0..=32)
                .map(|i| {
                    let a = start + (end - start) * i as f32 / 32.0;
                    let point = centre + vec2(a.cos(), a.sin()) * 8.0;
                    at(point.x, point.y)
                })
                .chain((0..=32).map(|i| {
                    // SVG scales the second radius (6.5) to half the chord, sqrt(50).
                    let a = -3.0 * std::f32::consts::FRAC_PI_4 - std::f32::consts::PI * i as f32 / 32.0;
                    at(15.0 + a.cos() * 50.0_f32.sqrt(), 9.0 + a.sin() * 50.0_f32.sqrt())
                }))
                .collect();
            p.add(egui::Shape::closed_line(points, stroke));
        }
        ThemeGlyph::Contrast => {
            p.circle_stroke(at(12.0, 12.0), 9.0 * scale, stroke);
            line(&[[12.0, 3.0], [12.0, 21.0]], 2.0);
            let points = (0..=32)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 32.0 * std::f32::consts::PI;
                    at(12.0 + a.cos() * 9.0, 12.0 + a.sin() * 9.0)
                })
                .collect();
            p.add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
        }
        ThemeGlyph::Monitor => {
            p.rect_stroke(Rect::from_min_max(at(3.0, 4.0), at(21.0, 16.0)), 2.0 * scale, stroke, StrokeKind::Middle);
            line(&[[8.0, 20.0], [16.0, 20.0]], 2.0);
            line(&[[12.0, 16.0], [12.0, 20.0]], 2.0);
        }
        ThemeGlyph::Carousel => {
            for (x, y, w, h, radius, opacity) in
                [(2.0, 6.0, 5.0, 12.0, 1.2, 0.55), (9.0, 4.0, 6.0, 16.0, 1.5, 1.0), (17.0, 6.0, 5.0, 12.0, 1.2, 0.55)]
            {
                p.rect_filled(Rect::from_min_size(at(x, y), vec2(w, h) * scale), radius * scale, ink.gamma_multiply(opacity));
            }
        }
        ThemeGlyph::Grid => {
            for x in [3.0, 13.0] {
                for y in [3.0, 13.0] {
                    p.rect_filled(Rect::from_min_size(at(x, y), egui::Vec2::splat(Tokens::get().space_8) * scale), 1.5 * scale, ink);
                }
            }
        }
        ThemeGlyph::Check => {
            line(&[[5.0, 12.0], [10.0, 17.0], [19.0, 7.0]], 3.2);
        }
        ThemeGlyph::Import => {
            line(&[[12.0, 4.0], [12.0, 15.0]], 2.0);
            line(&[[7.0, 10.0], [12.0, 15.0], [17.0, 10.0]], 2.0);
            line(&[[5.0, 20.0], [19.0, 20.0]], 2.0);
        }
        ThemeGlyph::Previous => line(&[[15.0, 5.0], [8.0, 12.0], [15.0, 19.0]], 2.4),
        ThemeGlyph::Next => line(&[[9.0, 5.0], [16.0, 12.0], [9.0, 19.0]], 2.4),
    }
}

pub(crate) struct ThemeSegment<'a> {
    label: &'a str,
    icon: ThemeGlyph,
    count: Option<usize>,
}
impl<'a> ThemeSegment<'a> {
    pub(crate) fn new(label: &'a str, icon: ThemeGlyph, count: Option<usize>) -> Self {
        Self { label, icon, count }
    }
}

pub(crate) fn theme_segments(ui: &mut Ui, key: &str, options: &[ThemeSegment<'_>], selected: Option<usize>, icon_only: bool) -> Option<usize> {
    let labels: Vec<_> = options.iter().map(|o| o.label).collect();
    super::settings_search::note_unlabelled(ui, &labels);
    if super::settings_search::querying(ui) && !super::settings_search::visible(ui, &labels) {
        return None;
    }
    let t = Tokens::get();
    let mut clicked = None;
    let icon_size = if key == "tone" || key == "contrast" { t.space_12 } else { t.space_14 };
    let widths: Vec<_> = options
        .iter()
        .map(|option| {
            let text = option.count.map(|n| format!("{} {n}", option.label)).unwrap_or_else(|| option.label.to_owned());
            let icon_width = if option.icon == ThemeGlyph::None { 0.0 } else { icon_size + t.space_6 };
            if icon_only { t.space_24 + t.space_10 } else { ContentWidth::name(ui, &text) + t.space_16 + icon_width }
        })
        .collect();
    // Reserve just the group's natural width. A nested horizontal Ui would reserve the
    // remaining row too, pushing the right-hand toolbar controls beyond the page.
    let (group, _) = ui.allocate_exact_size(vec2(widths.iter().sum(), t.control_height), Sense::hover());
    let mut x = group.left();
    for (index, option) in options.iter().enumerate() {
        let text = option.count.map(|n| format!("{} {n}", option.label)).unwrap_or_else(|| option.label.to_owned());
        let icon_size = if key == "tone" || key == "contrast" { t.space_12 } else { t.space_14 };
        let icon_width = if option.icon == ThemeGlyph::None { 0.0 } else { icon_size + t.space_6 };
        let width = widths.get(index).copied().unwrap_or_default();
        let r = Rect::from_min_size(pos2(x, group.top()), vec2(width, t.control_height));
        x += width;
        let response = ui.interact(r, ui.make_persistent_id((key, index)), Sense::click());
        let on = selected == Some(index);
        let rounding = egui::CornerRadius {
            nw: if index == 0 { t.radius_md } else { 0 },
            sw: if index == 0 { t.radius_md } else { 0 },
            ne: if index + 1 == options.len() { t.radius_md } else { 0 },
            se: if index + 1 == options.len() { t.radius_md } else { 0 },
        };
        let rounding = if options.len() == 1 { egui::CornerRadius::same((t.control_height / 2.0) as u8) } else { rounding };
        ui.painter().rect(
            r,
            rounding,
            if on {
                t.selected_bg
            } else if response.hovered() {
                t.hover
            } else {
                t.control_fill
            },
            Stroke::new(1.0, if on { t.form_primary } else { t.control_border }),
            StrokeKind::Inside,
        );
        let ink = if on { t.selected_ink } else { t.form_ink };
        let icon = Rect::from_center_size(
            if icon_only { r.center() } else { pos2(r.left() + t.space_8 + icon_size / 2.0, r.center().y) },
            vec2(icon_size, icon_size),
        );
        glyph(ui.painter(), icon, option.icon, ink);
        if !icon_only {
            ui.painter().text(
                pos2(r.left() + t.space_8 + icon_width, r.center().y),
                Align2::LEFT_CENTER,
                &text,
                FontId::proportional(t.font_label),
                ink,
            );
        }
        if response.has_focus() {
            ui.painter().rect_stroke(r.expand(t.space_2), rounding, Stroke::new(t.space_2, t.form_primary), StrokeKind::Outside);
        }
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, option.label));
        super::settings_search::remember(ui, option.label, r);
        super::settings_search::follow(ui, option.label, r);
        let visible = r.intersect(ui.clip_rect());
        if visible.is_positive() {
            solidcraft_ui_registry::publish_target(&format!("prefs:theme_{key}:{index}"), visible);
        }
        if response.clicked() {
            clicked = Some(index);
        }
        response.backing_on_hover_text(option.label).on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    clicked
}

/// Reserve the trailing control first; the leading groups wrap within the space left.
/// Only the mode/import row supplies a nonzero stacking threshold (about 460 pt).
pub(crate) fn theme_toolbar<L, R>(
    ui: &mut Ui,
    key: &str,
    trailing_width: f32,
    stack_below: f32,
    leading: impl FnOnce(&mut Ui) -> L,
    trailing: impl FnOnce(&mut Ui) -> R,
) -> (L, R) {
    let t = Tokens::get();
    let width = bounded_width(ui);
    let gap = t.space_10;
    let stacked = width < stack_below;
    let right_width = trailing_width.min(width);
    let left_width = if stacked { width } else { (width - right_width - gap).max(0.0) };
    let start = ui.next_widget_position();
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .id_salt((key, "leading"))
            .max_rect(Rect::from_min_size(start, vec2(left_width, t.control_height)))
            .layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true)),
    );
    left.set_clip_rect(left.clip_rect().intersect(Rect::from_x_y_ranges(start.x..=start.x + left_width, f32::NEG_INFINITY..=f32::INFINITY)));
    left.spacing_mut().item_spacing.x = t.space_8;
    let a = leading(&mut left);
    let left_height = left.min_size().y.max(t.control_height);
    let right_start = pos2(start.x + width - right_width, start.y + if stacked { left_height + gap } else { 0.0 });
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .id_salt((key, "trailing"))
            .max_rect(Rect::from_min_size(right_start, vec2(right_width, t.control_height)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let b = trailing(&mut right);
    let height = if stacked { left_height + gap + right.min_size().y } else { left_height.max(right.min_size().y) };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    solidcraft_ui_registry::publish_rect(&format!("prefs:theme_toolbar:{key}"), rect);
    (a, b)
}

fn bounded_width(ui: &Ui) -> f32 {
    let available = super::metrics::dimension(ui.available_width());
    if ui.is_visible() { available.min((ui.clip_rect().right() - ui.next_widget_position().x).max(0.0)) } else { available }
}

pub(crate) fn theme_import_button(ui: &mut Ui, label: &str) -> Response {
    super::settings_search::note_unlabelled(ui, &[label]);
    if super::settings_search::querying(ui) && !super::settings_search::visible(ui, &[label]) {
        return super::settings_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let width = ContentWidth::name(ui, label) + t.space_14 + t.space_6 + t.space_10 * 2.0;
    let (r, response) = ui.allocate_exact_size(vec2(width, t.control_height), Sense::click());
    ui.painter().rect(
        r,
        t.radius_md,
        if response.hovered() { t.hover } else { t.control_fill },
        Stroke::new(1.0, t.control_border),
        StrokeKind::Inside,
    );
    let icon = Rect::from_center_size(pos2(r.left() + t.space_10 + t.space_14 / 2.0, r.center().y), vec2(t.space_14, t.space_14));
    glyph(ui.painter(), icon, ThemeGlyph::Import, t.form_ink);
    ui.painter().text(pos2(icon.right() + t.space_6, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(t.font_label), t.form_ink);
    if response.has_focus() {
        ui.painter().rect_stroke(r.expand(t.space_2), t.radius_md, Stroke::new(t.space_2, t.form_primary), StrokeKind::Outside);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    super::settings_search::remember(ui, label, r);
    super::settings_search::follow(ui, label, r);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) struct ThemeSlot<'a> {
    pub title: &'a str,
    pub name: &'a str,
    pub hint: &'a str,
    pub aimed: bool,
    pub night: bool,
    pub width: f32,
    pub sample: ThemeSample,
}
pub(crate) fn theme_slot(ui: &mut Ui, slot: &ThemeSlot<'_>) -> Response {
    let t = Tokens::get();
    super::settings_search::note_unlabelled(ui, &[slot.title, slot.name, slot.hint]);
    if super::settings_search::querying(ui) && !super::settings_search::visible(ui, &[slot.title, slot.name, slot.hint]) {
        return super::settings_search::hidden_response(ui);
    }
    let (r, response) = ui.allocate_exact_size(vec2(slot.width.max(0.0), t.space_24 * 2.0 + t.space_16), Sense::click());
    ui.painter().rect(
        r,
        t.radius_lg,
        if slot.aimed { t.selected_bg } else { t.control_fill },
        Stroke::new(if slot.aimed { 2.0 } else { 1.0 }, if slot.aimed { t.form_primary } else { t.dialog_border }),
        StrokeKind::Inside,
    );
    let thumbnail = Rect::from_min_size(r.min + vec2(t.space_10, t.space_10), vec2(t.space_24 * 2.0 + t.space_16, t.space_24 * 2.0 - t.space_8));
    paint_theme_thumbnail_rounded(ui.painter(), thumbnail, &slot.sample, (f32::from(t.radius_lg) - if slot.aimed { 2.0 } else { 1.0 }).max(0.0));
    let x = thumbnail.right() + t.space_10;
    let icon = Rect::from_min_size(pos2(x, r.top() + t.space_10), vec2(t.space_12, t.space_12));
    glyph(ui.painter(), icon, if slot.night { ThemeGlyph::Moon } else { ThemeGlyph::Sun }, t.form_dim);
    let p = ui.painter().with_clip_rect(r.shrink(t.space_8).intersect(ui.clip_rect()));
    p.text(pos2(icon.right() + t.space_4, icon.center().y), Align2::LEFT_CENTER, slot.title, FontId::proportional(t.font_small), t.form_dim);
    p.text(pos2(x, r.bottom() - t.space_16 - t.space_4), Align2::LEFT_CENTER, slot.name, crate::fonts::semibold(t.font_label), t.form_ink);
    p.text(pos2(r.right() - t.space_10, r.center().y), Align2::RIGHT_CENTER, slot.hint, FontId::proportional(t.font_small), t.form_dim);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), slot.aimed, slot.title));
    super::settings_search::remember(ui, slot.title, r);
    super::settings_search::follow(ui, slot.title, r);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub(crate) struct ThemeCard<'a> {
    pub name: &'a str,
    pub note: &'a str,
    pub options: Option<&'a str>,
    pub sample: ThemeSample,
    pub selected: bool,
    pub dark: bool,
    pub high: bool,
    pub day: bool,
    pub night: bool,
}
pub(crate) fn theme_card(ui: &mut Ui, card: &ThemeCard<'_>, width: f32) -> Response {
    let t = Tokens::get();
    let inner_radius = (f32::from(t.radius_lg) - if card.selected { 2.0 } else { 1.0 }).max(0.0);
    let response = picture_card(
        ui,
        &Card {
            label: card.name,
            sub: Some(card.note),
            selected: card.selected,
            enabled: true,
            options: None,
            check: false,
            aspect: 124.0 / 76.0,
            figure: &|p, r| paint_theme_thumbnail_rounded(p, r, &card.sample, inner_radius),
        },
        width,
    );
    if !response.rect.is_positive() {
        return response;
    }
    let r = response.rect;
    let size = t.space_16 + t.space_2;
    let badge_width = if card.high { t.space_10 * 2.0 + t.space_2 + t.space_4 * 2.0 } else { size };
    let badge = Rect::from_min_size(r.min + vec2(t.space_8, t.space_8), vec2(badge_width, size));
    let dark = Tokens::dark();
    ui.painter().rect_filled(badge, size / 2.0, dark.panel.gamma_multiply(0.85));
    let tone = Rect::from_center_size(pos2(badge.left() + t.space_4 + t.space_10 / 2.0, badge.center().y), vec2(t.space_10, t.space_10));
    glyph(ui.painter(), tone, if card.dark { ThemeGlyph::Moon } else { ThemeGlyph::Sun }, dark.text);
    if card.high {
        glyph(ui.painter(), tone.translate(vec2(t.space_12, 0.0)), ThemeGlyph::Contrast, dark.text);
    }
    let mut x = r.right() - t.space_8 - size / 2.0;
    for kind in [
        card.night.then_some(ThemeGlyph::Moon),
        card.day.then_some(ThemeGlyph::Sun),
        (card.selected && !card.day && !card.night).then_some(ThemeGlyph::Check),
    ]
    .into_iter()
    .flatten()
    {
        let centre = pos2(x, badge.center().y);
        ui.painter().circle_filled(centre, size / 2.0, t.form_primary);
        glyph(ui.painter(), Rect::from_center_size(centre, vec2(t.space_10 + 1.0, t.space_10 + 1.0)), kind, t.on_accent);
        x -= size + t.space_2;
    }
    if let Some(help) = card.options {
        let centre = pos2(r.right() - t.space_8 - size / 2.0, r.top() + t.space_4 + (width - t.space_8) * 76.0 / 124.0 - size / 2.0);
        ui.painter().circle_filled(centre, size / 2.0, t.form_primary);
        crate::icons::paint_ink(ui.painter(), Rect::from_center_size(centre, vec2(t.space_12, t.space_12)), "settings", t.on_accent);
        return response.backing_on_hover_text(help);
    }
    response
}

fn browser_frame<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let container = Container::new(ui.ctx(), Surface::Navigation);
    let frame = super::preferences::settings_card_frame(ui).inner_margin(0);
    let border = frame.stroke.width;
    let width = bounded_width(ui);
    container.show_frame(ui, frame, ui.max_rect(), |ui| {
        ui.set_width((width - border * 2.0).max(0.0));
        add(ui)
    })
}

pub(crate) fn theme_empty(ui: &mut Ui, message: &str, clear: &str, reset: impl FnOnce()) {
    browser_frame(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.set_min_height(Tokens::get().space_24 * 5.0);
        ui.vertical_centered(|ui| {
            ui.add_space(Tokens::get().space_24);
            ui.label(message);
            if ui.link(clear).clicked() {
                reset();
            }
        });
        solidcraft_ui_registry::publish_rect("prefs:theme_empty", ui.min_rect());
    });
}

/// A clipped row with overlaid fades, or five equal columns with vertical scrolling.
pub(crate) fn theme_browser(ui: &mut Ui, id: &str, count: usize, grid: bool, reveal: Option<usize>, mut item: impl FnMut(&mut Ui, usize, f32)) {
    let t = Tokens::get();
    let fill = super::preferences::settings_card_frame(ui).fill;
    browser_frame(ui, |ui| {
        let width = bounded_width(ui);
        let gap = t.space_8;
        let card_width = if grid { ((width - t.space_10 * 2.0 - gap * 4.0) / 5.0).max(1.0) } else { t.space_24 * 5.0 - t.space_2 };
        let card_height = (card_width - t.space_8) * 76.0 / 124.0 + t.space_16 + t.space_14 + t.space_8 + t.space_6;
        if grid {
            egui::ScrollArea::vertical()
                .id_salt((id, "grid"))
                .auto_shrink([false, false])
                .max_width(width)
                .max_height((card_height + gap) * 2.5)
                .show(ui, |ui| {
                    let card_width = ((ui.available_width() - t.space_10 * 2.0 - gap * 4.0) / 5.0).max(1.0);
                    ui.add_space(t.space_10);
                    for row in (0..count).step_by(5) {
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            ui.spacing_mut().item_spacing.y = gap;
                            ui.allocate_exact_size(vec2((t.space_10 - gap).max(0.0), 0.0), Sense::hover());
                            for index in row..(row + 5).min(count) {
                                ui.allocate_ui_with_layout(vec2(card_width, card_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                                    item(ui, index, card_width);
                                    if reveal == Some(index) {
                                        ui.scroll_to_rect(ui.min_rect(), None);
                                    }
                                });
                            }
                        });
                    }
                    ui.add_space(t.space_10);
                });
            return;
        }
        let stride = card_width + gap;
        let padding = t.space_10;
        let content_width = count as f32 * stride - gap + padding * 2.0;
        let key = ui.make_persistent_id((id, "offset"));
        let mut offset = ui.data(|d| d.get_temp::<f32>(key)).unwrap_or_default();
        let max = (content_width - width).max(0.0);
        if let Some(index) = reveal {
            let start = padding + index as f32 * stride;
            if start < offset + padding {
                offset = start - padding;
            } else if start + card_width + padding > offset + width {
                offset = start + card_width + padding - width;
            }
        }
        offset = offset.clamp(0.0, max);
        let out = egui::ScrollArea::horizontal()
            .id_salt((id, "carousel"))
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            // egui normally maps vertical wheels to a horizontal-only scroll area.
            // Handle the horizontal axis below, leaving vertical input for the page.
            .scroll_source(egui::scroll_area::ScrollSource { mouse_wheel: false, ..Default::default() })
            .horizontal_scroll_offset(offset)
            .max_width(width)
            .max_height(card_height + padding * 2.0)
            .show(ui, |ui| {
                // A single reservation avoids implicit layout spacing below the row.
                // The end spacers scroll with the cards, so mid-scroll clipping still
                // reaches the box edges while both resting ends have equal padding.
                let (content, _) = ui.allocate_exact_size(vec2(content_width, card_height + padding * 2.0), Sense::hover());
                for index in 0..count {
                    let at = content.min + vec2(padding + index as f32 * stride, padding);
                    let mut card_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt((id, index))
                            .max_rect(Rect::from_min_size(at, vec2(card_width, card_height)))
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    item(&mut card_ui, index, card_width);
                }
            });
        offset = out.state.clamped_offset().x;
        let r = out.inner_rect;
        let hovered = ui.rect_contains_pointer(r);
        let wheel = if hovered { super::wheel::take_horizontal_wheel(ui, offset, max) } else { 0.0 };
        let horizontal_wheel = wheel != 0.0;
        if horizontal_wheel {
            offset = (offset - wheel).clamp(0.0, max);
        }
        let time_key = key.with("last_scroll");
        let now = ui.input(|i| i.time);
        if horizontal_wheel || ui.input(|i| i.pointer.any_down()) {
            ui.data_mut(|d| d.insert_temp(time_key, now));
        } else if let Some(last) = ui.data(|d| d.get_temp::<f64>(time_key)) {
            if now - last > 0.15 {
                offset = ((offset / stride).round() * stride).clamp(0.0, max);
                ui.data_mut(|d| d.remove::<f64>(time_key));
            } else {
                ui.ctx().request_repaint();
            }
        }
        for (left, present) in [(true, offset > 0.5), (false, offset < max - 0.5)] {
            if !present {
                continue;
            }
            let fade_width = (t.space_24 * 3.0 + t.space_12).min(r.width() / 2.0);
            let edge = if left { r.left() } else { r.right() };
            let end = edge + if left { fade_width } else { -fade_width };
            let mut mesh = egui::Mesh::default();
            let clear = fill.gamma_multiply(0.0);
            for (x, color) in [(edge, fill), (end, clear)] {
                mesh.colored_vertex(pos2(x, r.top()), color);
                mesh.colored_vertex(pos2(x, r.bottom()), color);
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(1, 3, 2);
            ui.painter().with_clip_rect(r).add(egui::Shape::mesh(mesh));
            let size = t.space_24 + t.space_4;
            let centre = pos2(edge + if left { t.space_8 + size / 2.0 } else { -t.space_8 - size / 2.0 }, r.center().y);
            let arrow = Rect::from_center_size(centre, vec2(size, size));
            let response = ui.interact(arrow, key.with(left), Sense::click());
            let shadow = t.shell_shadow();
            ui.painter().add(shadow.as_shape(arrow, egui::CornerRadius::same((size / 2.0) as u8)));
            ui.painter().circle(centre, size / 2.0, if response.hovered() { t.hover } else { t.control_fill }, Stroke::new(1.0, t.dialog_border));
            let icon = Rect::from_center_size(centre, vec2(t.space_14, t.space_14));
            glyph(ui.painter(), icon, if left { ThemeGlyph::Previous } else { ThemeGlyph::Next }, t.form_ink);
            if response.has_focus() {
                ui.painter().circle_stroke(centre, size / 2.0 + t.space_2, Stroke::new(t.space_2, t.form_primary));
            }
            let label = solidcraft_ui_text::tr(if left { "prefs.appearance.previous_themes" } else { "prefs.appearance.next_themes" });
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
            solidcraft_ui_registry::publish_target(if left { "prefs:theme_previous" } else { "prefs:theme_next" }, arrow);
            if response.clicked() {
                // A screenful: the cards that fit in view, at least one (#11434).
                let page = (r.width() / stride).floor().max(1.0) * stride;
                offset = (offset + if left { -page } else { page }).clamp(0.0, max);
            }
            response.backing_on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        if (offset - out.state.clamped_offset().x).abs() > 0.1 {
            ui.ctx().request_repaint();
        }
        ui.data_mut(|d| d.insert_temp(key, offset));
        solidcraft_ui_registry::publish_rect("prefs:theme_viewport", r);
    });
}
