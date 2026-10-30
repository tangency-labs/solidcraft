//! Shared preference compositions and explanatory figures. Figures are cheap painter content,
//! never additional application viewports; every colour and control metric comes from tokens.
use super::{metrics::dimension, text::paint_text};
use crate::{
    scenario,
    theme::{ColorVision, Tokens},
};
use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, pos2, vec2};

pub(super) const PAGE_SCROLL_GUTTER: &str = "sc_page_scroll_gutter";

/// Page scroller keeps content aligned while centring its bar in the window's right inset.
/// Nested pages (including Shortcuts) inherit the same gutter and height contract.
pub(crate) fn settings_scroll<R>(ui: &mut Ui, id: impl egui::AsIdSalt, add: impl FnOnce(&mut Ui) -> R) -> egui::scroll_area::ScrollAreaOutput<R> {
    let t = Tokens::get();
    let gutter = ui.stack().iter().find_map(|node| node.tags().get_downcast::<f32>(PAGE_SCROLL_GUTTER)).copied().unwrap_or(0.0);
    let width = dimension(ui.available_width());
    let height = if ui.available_height().is_finite() { dimension(ui.available_height()) } else { t.control_height * 16.0 };
    let bar = ui.spacing().scroll.floating_width;
    let extension = if gutter > 0.0 { gutter / 2.0 + bar / 2.0 } else { 0.0 };
    let host = Rect::from_min_size(ui.next_widget_position(), vec2(width + extension, height));
    ui.scope_builder(egui::UiBuilder::new().max_rect(host).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
        if gutter > 0.0 {
            let scroll = &mut ui.style_mut().spacing.scroll;
            scroll.floating = true;
            scroll.floating_allocated_width = 0.0;
            scroll.bar_outer_margin = 0.0;
        }
        let mut scroll = egui::ScrollArea::vertical().id_salt(id).max_height(height).auto_shrink([false, false]).show(ui, |ui| {
            if gutter > 0.0 {
                // The scrollbar host extends into the gutter. Its content must neither use
                // that extension nor grow the page/window when a child requests extra width.
                let bounds = Rect::from_min_size(ui.next_widget_position(), vec2(width, ui.available_height()));
                let mut content = ui.new_child(egui::UiBuilder::new().max_rect(bounds));
                let clip = content.clip_rect();
                content.set_clip_rect(clip.intersect(Rect::from_min_max(pos2(bounds.left(), clip.top()), pos2(bounds.right(), clip.bottom()))));
                let result = add(&mut content);
                ui.allocate_exact_size(vec2(width, content.min_size().y), Sense::hover());
                result
            } else {
                add(ui)
            }
        });
        // Consumers publish the page's content well, not the extended scrollbar host.
        if gutter > 0.0 {
            scroll.inner_rect.max.x = scroll.inner_rect.left() + width;
        }
        scroll
    })
    .inner
}

pub(crate) fn settings_heading(ui: &mut Ui, title: &str, description: &str) {
    if crate::prefs_search::querying(ui) {
        return;
    }
    let t = Tokens::get();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = t.space_3;
        let (rect, response) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_16 + t.space_3), Sense::hover());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), title));
        paint_text(ui, rect, title, crate::fonts::semibold(t.font_intro), t.form_ink, false);
        super::text(ui, description, super::Text::Hint("settings_heading"));
    });
}

/// Settings reuse the same disclosure, uppercase typography and rule as the design system.
pub(crate) fn settings_section<R>(ui: &mut Ui, key: &str, title: &str, mut add: impl FnMut(&mut Ui) -> R) -> Option<R> {
    if crate::prefs_search::querying(ui) {
        crate::prefs_search::begin_section(ui, title);
        if !ui.is_visible() {
            return Some(add(ui));
        }
        if crate::prefs_search::shows_all(ui) {
            let mut child = crate::prefs_search::detached_child(ui, ("prefs-search-section", key), false);
            let result = add(&mut child);
            crate::prefs_search::place_search_child(ui, &child);
            return Some(result);
        }
        let mark = crate::prefs_search::search_layout_mark(ui);
        let before = crate::prefs_search::hits(ui);
        let (gained, missed) = {
            let mut probe = crate::prefs_search::detached_child(ui, ("prefs-search-section-probe", key), true);
            let missed = add(&mut probe);
            (crate::prefs_search::hits(ui) > before, missed)
        };
        crate::prefs_search::restore_search_layout(ui, mark);
        if !gained {
            return Some(missed);
        }
        let mut child = crate::prefs_search::detached_child(ui, ("prefs-search-section", key), false);
        let result = add(&mut child);
        crate::prefs_search::place_search_child(ui, &child);
        return Some(result);
    }
    let t = Tokens::get();
    ui.add_space(t.space_8);
    super::section(ui, key, title, true, |ui| {
        if let Some(rect) = scenario::rect_of(&format!("dialog:section:{title}")) {
            scenario::publish_rect(&format!("prefs:section:{key}"), rect);
        }
        ui.spacing_mut().item_spacing.y = t.space_8;
        add(ui)
    })
}

/// Full-width settings detail group. Container owns its surface; content cannot shrink the card.
pub(crate) fn settings_card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let t = Tokens::get();
    let container = super::Container::new(ui.ctx(), super::Surface::Navigation);
    let frame = container
        .frame(ui.ctx(), t.dialog_footer, crate::frame::Side::Left)
        .inner_margin(egui::Margin::symmetric(t.space_12 as i8, t.space_10 as i8));
    container.show_frame(ui, frame, ui.max_rect(), |ui| {
        ui.set_min_width(dimension(ui.available_width()));
        add(ui)
    })
}

/// Only the four palette roles needed by a swatch, avoiding a copy of the full theme.
#[derive(Clone, Copy)]
pub(crate) struct ThemeSwatch {
    background: Color32,
    panel: Color32,
    accent: Color32,
    dim: Color32,
    radius: u8,
}
impl From<Tokens> for ThemeSwatch {
    fn from(t: Tokens) -> Self {
        Self { background: t.viewport_top, panel: t.panel, accent: t.accent, dim: t.text_dim, radius: t.control_radius }
    }
}

/// Controls and one explanatory specimen side by side, stacked on narrow viewports.
pub(crate) fn settings_pair<R>(ui: &mut Ui, controls: impl FnOnce(&mut Ui) -> R, preview: impl FnOnce(&mut Ui, R)) {
    if crate::prefs_search::suppress_chrome(ui) {
        let _ = controls(ui);
        return;
    }
    let t = Tokens::get();
    let preview_width = t.form_card_width - t.space_8;
    let gap = t.space_16 + t.space_4;
    let width = dimension(ui.available_width());
    if width < preview_width * 2.0 + gap {
        let state = controls(ui);
        preview(ui, state);
    } else {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let state = ui
                .allocate_ui_with_layout(vec2(width - preview_width - gap, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = t.space_6;
                    controls(ui)
                })
                .inner;
            ui.allocate_ui_with_layout(vec2(preview_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| preview(ui, state));
        });
    }
}

/// A horizontally scrollable, draggable tile rail with its import action pinned outside it.
/// The hidden scrollbar never reserves vertical space; a vertical wheel also moves the rail.
pub(crate) fn theme_rail(ui: &mut Ui, tiles: impl FnOnce(&mut Ui), import: impl FnOnce(&mut Ui)) {
    if crate::prefs_search::suppress_chrome(ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = Tokens::get().space_8;
            tiles(ui);
        });
        import(ui);
        return;
    }
    let t = Tokens::get();
    let action_width = t.form_label_width + t.space_24 + t.space_6;
    let rail_width = (dimension(ui.available_width()) - action_width - t.space_8).max(0.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = t.space_8;
        ui.allocate_ui_with_layout(vec2(rail_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.style_mut().always_scroll_the_only_direction = true;
            // This rail paints its own fully opaque, page-coloured edge cues.
            ui.spacing_mut().scroll.fade.strength = 0.0;
            let mut rail = egui::ScrollArea::horizontal()
                .id_salt("settings_theme_rail")
                .max_width(rail_width)
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(egui::scroll_area::ScrollSource { drag: egui::scroll_area::DragScroll::Always, ..Default::default() })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = t.space_8;
                        tiles(ui);
                    });
                });
            let max_offset = (rail.content_size.x - rail.inner_rect.width()).max(0.0);
            let (left, right) = theme_rail_cues(rail.state.offset.x, max_offset);
            let arrow_size = t.control_height.min(rail.inner_rect.width() / 2.0);
            for (visible, direction, label) in [(left, -1.0, "Scroll left"), (right, 1.0, "Scroll right")] {
                if !visible || arrow_size <= 0.0 {
                    continue;
                }
                // The horizontal host starts with zero requested height; use the measured
                // cards' height rather than the scroll area's initial viewport height.
                let rail_rect = Rect::from_min_size(rail.inner_rect.min, vec2(rail.inner_rect.width(), rail.content_size.y));
                theme_rail_fade(ui, rail_rect, direction);
                let center = pos2(
                    if direction < 0.0 { rail.inner_rect.left() + arrow_size / 2.0 } else { rail.inner_rect.right() - arrow_size / 2.0 },
                    rail.inner_rect.top() + rail.content_size.y / 2.0,
                );
                let rect = Rect::from_center_size(center, vec2(arrow_size, t.control_height));
                if theme_rail_arrow(ui, rect, direction, label).clicked() {
                    rail.state.offset.x = (rail.state.offset.x + direction * (action_width + t.space_8)).clamp(0.0, max_offset);
                    rail.state.store(ui.ctx(), rail.id);
                    ui.ctx().request_repaint();
                }
            }
        });
        ui.allocate_ui_with_layout(vec2(action_width, 0.0), egui::Layout::top_down(egui::Align::Min), import);
    });
}

/// Ignore subpixel rounding at either end so cues do not flicker on a fully visible rail.
pub(super) fn theme_rail_cues(offset: f32, max_offset: f32) -> (bool, bool) {
    (offset > 0.5, max_offset - offset > 0.5)
}

/// Preferences pages inherit the Window card's opaque dialog_body surface.
/// Paint after the cards and before the arrows so the outer edge fully conceals content.
fn theme_rail_fade(ui: &Ui, rail: Rect, direction: f32) {
    let t = Tokens::get();
    let width = (t.space_24 * 2.0 + t.space_8).min(rail.width() / 2.0);
    let outer = if direction < 0.0 { rail.left() } else { rail.right() };
    let inner = outer - direction * width;
    let mut mesh = egui::Mesh::default();
    for (x, color) in [(outer, t.dialog_body), (inner, Color32::TRANSPARENT)] {
        mesh.colored_vertex(pos2(x, rail.top()), color);
        mesh.colored_vertex(pos2(x, rail.bottom()), color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    ui.painter().with_clip_rect(ui.clip_rect().intersect(rail)).add(mesh);
}

pub(super) fn theme_rail_arrow(ui: &mut Ui, rect: Rect, direction: f32, label: &str) -> Response {
    let t = Tokens::get();
    let response = ui.interact(rect, ui.make_persistent_id(("theme_rail_arrow", label)), Sense::click());
    if response.hovered() {
        ui.painter().circle_filled(rect.center(), rect.width().min(rect.height()) / 2.0, t.hover);
    }
    let ink = if response.hovered() { t.form_ink } else { t.form_ink.gamma_multiply(0.65) };
    let icon = Rect::from_center_size(rect.center(), vec2(t.space_16, t.space_16));
    if direction < 0.0 {
        crate::icons::paint_ink_mirrored(ui.painter(), icon, "chevron_right", ink);
    } else {
        crate::icons::paint_ink(ui.painter(), icon, "chevron_right", ink);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect, t.control_radius, Stroke::new(t.space_2, t.form_primary), egui::StrokeKind::Inside);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), crate::i18n::tr_label(label)));
    scenario::publish_target(&format!("prefs:theme_rail:{label}"), rect);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(crate::i18n::tr_label(label))
}

/// Dashed import tile matches the swatch tiles, while remaining permanently visible.
pub(crate) fn theme_import_tile(ui: &mut Ui, label: &str, note: &str) -> Response {
    if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[label, note]) {
        return crate::prefs_search::hidden_response(ui);
    }
    let t = Tokens::get();
    let width = t.form_label_width + t.space_24 + t.space_6;
    let height = t.control_height + t.space_6 * 2.0 + t.space_4 + t.space_16 * 2.0;
    let (r, response) = ui.allocate_exact_size(vec2(width.min(dimension(ui.available_width())), height), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(r, t.radius_lg, t.hover);
    }
    let mut outline = rounded_perimeter(r.shrink(t.space_2 / 2.0), f32::from(t.radius_lg));
    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }
    ui.painter().extend(egui::Shape::dashed_line(&outline, Stroke::new(t.space_3 / 2.0, t.control_border), t.space_3, t.space_3));
    let icon = Rect::from_center_size(pos2(r.center().x, r.top() + t.space_16), vec2(t.space_16, t.space_16));
    crate::icons::paint_ink(ui.painter(), icon, "import_tray", t.form_dim);
    centred_text(
        ui.painter(),
        Rect::from_min_size(pos2(r.left(), r.top() + t.space_24 + t.space_4), vec2(r.width(), t.space_16)),
        label,
        FontId::proportional(t.font_label),
        t.form_dim,
    );
    centred_text(
        ui.painter(),
        Rect::from_min_size(pos2(r.left(), r.top() + t.space_24 + t.space_4 + t.space_16), vec2(r.width(), t.space_16)),
        note,
        FontId::proportional(t.font_small),
        t.form_dim,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    scenario::publish_target(&format!("dialog:{label}"), r);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Compact informational state badge for settings; never an interactive control.
pub(crate) fn settings_badge(ui: &mut Ui, text: &str, warning: bool) {
    if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[text]) {
        return;
    }
    let t = Tokens::get();
    let ink =
        if warning { if crate::theme::contrast_ratio(t.warning, t.dialog_body) >= 4.5 { t.warning } else { t.form_ink } } else { t.form_success };
    let fill = if warning { t.warning.gamma_multiply(0.1) } else { t.form_success_fill };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), crate::fonts::semibold(t.font_small), ink);
    let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + t.space_16, t.space_24), Sense::hover());
    ui.painter().rect_filled(r, t.radius_xl, fill);
    ui.painter().galley(r.center() - galley.size() / 2.0, galley, ink);
}

#[derive(Clone, Copy)]
pub(crate) enum PreferenceFigure {
    Theme { palette: ThemeSwatch, system: bool },
    Pivot(usize),
    Quality(usize),
    Antialiasing(u32),
    ContactShading(bool),
    UpAxis(bool),
}

/// Reusable illustrated radio card. Width comes from the figure, and wrapping is owned by
/// the parent; labels remain accessible even when translations need an ellipsis.
pub(crate) fn option_card(ui: &mut Ui, label: &str, note: Option<&str>, selected: bool, enabled: bool, figure: PreferenceFigure) -> Response {
    if crate::prefs_search::querying(ui) {
        let show = match note {
            Some(note) => crate::prefs_search::visible(ui, &[label, note]),
            None => crate::prefs_search::visible(ui, &[label]),
        };
        if !show {
            return crate::prefs_search::hidden_response(ui);
        }
    }
    let t = Tokens::get();
    let theme = matches!(figure, PreferenceFigure::Theme { .. });
    let width = match figure {
        PreferenceFigure::Theme { .. } => t.form_label_width + t.space_24 + t.space_6,
        PreferenceFigure::Antialiasing(_) => t.space_24 * 3.0 + t.space_16,
        _ => t.form_label_width + t.space_24 * 2.0 + t.space_6,
    };
    let preview_height = if theme {
        t.control_height
    } else if matches!(figure, PreferenceFigure::Antialiasing(_)) {
        t.space_24 * 3.0
    } else {
        t.space_24 * 3.0 + t.space_3
    };
    let label_height = if matches!(figure, PreferenceFigure::ContactShading(_)) { t.space_16 * 2.0 } else { t.space_16 };
    let height = preview_height + t.space_6 * 2.0 + t.space_4 + label_height + if note.is_some() { t.space_16 } else { 0.0 };
    let enabled = enabled && ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(
        vec2(if theme { width } else { width.min(dimension(ui.available_width())) }, height),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hover = enabled && response.hovered();
    let dim = if selected { t.readable(t.form_dim, &[t.selected_bg]) } else { t.form_dim };
    let ink = if !enabled {
        dim
    } else if selected {
        t.selected_ink
    } else {
        t.form_ink
    };
    ui.painter().rect(
        rect,
        t.radius_lg,
        if selected {
            t.selected_bg
        } else if hover {
            t.hover
        } else {
            t.control_fill
        },
        if selected { Stroke::new(2.0, t.form_primary) } else { t.control_border_stroke(t.control_border) },
        egui::StrokeKind::Inside,
    );
    let mut preview = Rect::from_min_size(rect.min + vec2(t.space_6, t.space_6), vec2((rect.width() - t.space_12).max(0.0), preview_height));
    if matches!(figure, PreferenceFigure::Antialiasing(_)) {
        preview = Rect::from_center_size(preview.center(), vec2(preview.height(), preview.height()));
    }
    paint_preference_figure(ui.painter(), preview, figure, enabled);
    let label_rect =
        Rect::from_min_size(pos2(rect.left() + t.space_4, preview.bottom() + t.space_4), vec2((rect.width() - t.space_8).max(0.0), label_height));
    centred_text_rows(
        ui.painter(),
        label_rect,
        label,
        if selected { crate::fonts::semibold(t.font_label) } else { FontId::proportional(t.font_label) },
        ink,
        if matches!(figure, PreferenceFigure::ContactShading(_)) { 2 } else { 1 },
    );
    if let Some(note) = note {
        centred_text(
            ui.painter(),
            Rect::from_min_size(pos2(label_rect.left(), label_rect.bottom()), vec2(label_rect.width(), t.space_16)),
            note,
            FontId::proportional(t.font_small),
            dim,
        );
    }
    if selected {
        let centre = pos2(rect.right() - t.space_6 - t.space_8, rect.top() + t.space_6 + t.space_8);
        ui.painter().circle_filled(centre, t.space_8, t.form_primary);
        let ink = t.on_accent;
        ui.painter().line_segment([centre + vec2(-t.space_4, 0.0), centre + vec2(-t.space_2, t.space_2)], Stroke::new(2.0, ink));
        ui.painter().line_segment([centre + vec2(-t.space_2, t.space_2), centre + vec2(t.space_4, -t.space_3)], Stroke::new(2.0, ink));
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, enabled, selected, label));
    scenario::publish_target(&format!("prefs:option:{label}"), rect);
    let response = response.on_hover_text(label);
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

fn centred_text(p: &egui::Painter, rect: Rect, label: &str, font: FontId, ink: Color32) {
    centred_text_rows(p, rect, label, font, ink, 1);
}

fn centred_text_rows(p: &egui::Painter, rect: Rect, label: &str, font: FontId, ink: Color32, rows: usize) {
    let mut job = egui::text::LayoutJob::simple(label.to_owned(), font, ink, rect.width());
    job.wrap.max_rows = rows;
    job.halign = egui::Align::Center;
    let galley = p.layout_job(job);
    p.with_clip_rect(rect).galley(rect.center() - galley.rect.center().to_vec2(), galley, ink);
}

fn point(r: Rect, x: f32, y: f32) -> Pos2 {
    pos2(r.left() + r.width() * x, r.top() + r.height() * y)
}

fn paint_preference_figure(p: &egui::Painter, rect: Rect, figure: PreferenceFigure, enabled: bool) {
    if !p.clip_rect().intersects(rect) {
        return;
    }
    let t = Tokens::get();
    let p = p.with_clip_rect(rect);
    let alpha = if enabled { 1.0 } else { 0.4 };
    let accent = t.form_primary.gamma_multiply(alpha);
    if let PreferenceFigure::Theme { palette, system } = figure {
        theme_swatch(&p, rect, palette, false);
        if system {
            theme_swatch(&p, rect, Tokens::dark().into(), true);
        }
        return;
    }
    p.rect_filled(rect, t.control_radius, t.viewport_top);
    t.container_rect(&p, rect, t.control_radius, egui::Color32::TRANSPARENT, t.dialog_rule);
    match figure {
        PreferenceFigure::Theme { .. } => {}
        PreferenceFigure::Quality(sides) => {
            let n = sides.clamp(3, 96);
            let centre = rect.center();
            let radius = rect.height() * 0.34;
            let guide: Vec<_> = (0..=96)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 96.0;
                    centre + vec2(a.cos(), a.sin()) * radius
                })
                .collect();
            p.extend(egui::Shape::dashed_line(&guide, t.control_border_stroke(t.control_border), t.space_2, t.space_3));
            let points = (0..n)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / n as f32;
                    centre + vec2(a.cos(), a.sin()) * radius
                })
                .collect();
            p.add(egui::Shape::convex_polygon(points, accent.gamma_multiply(0.15), Stroke::new(1.5, accent)));
        }
        PreferenceFigure::Antialiasing(samples) => {
            let n = 10;
            let s = samples.clamp(1, 8);
            for y in 0..n {
                for x in 0..n {
                    let mut covered = 0;
                    for sy in 0..s {
                        for sx in 0..s {
                            let px = (x as f32 + (sx as f32 + 0.5) / s as f32) / n as f32;
                            let py = (y as f32 + (sy as f32 + 0.5) / s as f32) / n as f32;
                            if py > 0.3 + px * 0.35 {
                                covered += 1;
                            }
                        }
                    }
                    let cell = Rect::from_min_max(
                        point(rect, x as f32 / n as f32, y as f32 / n as f32),
                        point(rect, (x + 1) as f32 / n as f32, (y + 1) as f32 / n as f32),
                    );
                    p.rect_filled(cell, 0, t.viewport_top.lerp_to_gamma(accent, covered as f32 / (s * s) as f32));
                }
            }
        }
        PreferenceFigure::UpAxis(y_up) => super::preferences_figures::paint_orientation(&p, rect, y_up),
        PreferenceFigure::Pivot(pivot) => super::preferences_figures::paint_orbit(&p, rect, pivot),
        PreferenceFigure::ContactShading(on) => super::preferences_figures::paint_contact_shading(&p, rect, on),
    }
}

/// Clockwise rounded boundary shared by dashed outlines and clipped palette specimens.
pub(super) fn rounded_perimeter(r: Rect, radius: f32) -> Vec<Pos2> {
    let radius = radius.min(r.width() / 2.0).min(r.height() / 2.0).max(0.0);
    let corners = [
        (pos2(r.right() - radius, r.top() + radius), -std::f32::consts::FRAC_PI_2),
        (pos2(r.right() - radius, r.bottom() - radius), 0.0),
        (pos2(r.left() + radius, r.bottom() - radius), std::f32::consts::FRAC_PI_2),
        (pos2(r.left() + radius, r.top() + radius), std::f32::consts::PI),
    ];
    corners
        .into_iter()
        .flat_map(|(centre, start)| {
            (0..=8).map(move |step| {
                let angle = start + std::f32::consts::FRAC_PI_2 * step as f32 / 8.0;
                centre + vec2(angle.cos(), angle.sin()) * radius
            })
        })
        .collect()
}

fn clip_half_plane(vertices: Vec<Pos2>, side: impl Fn(Pos2) -> f32) -> Vec<Pos2> {
    let Some(mut previous) = vertices.last().copied() else { return vertices };
    let mut clipped = Vec::with_capacity(vertices.len() + 2);
    for current in vertices {
        let a = side(previous);
        let b = side(current);
        if (a >= 0.0) != (b >= 0.0) {
            clipped.push(previous + (current - previous) * (a / (a - b)));
        }
        if b >= 0.0 {
            clipped.push(current);
        }
        previous = current;
    }
    clipped
}

fn theme_swatch(p: &egui::Painter, r: Rect, c: ThemeSwatch, diagonal: bool) {
    let boundary = rounded_perimeter(r, f32::from(c.radius));
    let fill = |mut vertices: Vec<Pos2>, color: Color32| {
        if diagonal {
            vertices = clip_half_plane(vertices, |v| (v.x - r.left()) / r.width() + (v.y - r.top()) / r.height() - 1.0);
        }
        for (index, &a) in boundary.iter().enumerate() {
            let b = boundary.get(index + 1).copied().unwrap_or_else(|| boundary.first().copied().unwrap_or(a));
            vertices = clip_half_plane(vertices, |v| (b.x - a.x) * (v.y - a.y) - (b.y - a.y) * (v.x - a.x));
            if vertices.is_empty() {
                break;
            }
        }
        if vertices.len() >= 3 {
            p.add(egui::Shape::convex_polygon(vertices, color, Stroke::NONE));
        }
    };
    let rect =
        |a: (f32, f32), b: (f32, f32), color| fill(vec![point(r, a.0, a.1), point(r, b.0, a.1), point(r, b.0, b.1), point(r, a.0, b.1)], color);
    rect((0.0, 0.0), (1.0, 1.0), c.background);
    rect((0.0, 0.0), (0.34, 1.0), c.panel);
    rect((0.07, 0.29), (0.27, 0.40), c.dim);
    rect((0.07, 0.54), (0.20, 0.65), c.dim);
    let centre = point(r, 0.66, 0.5);
    let radius = r.height() * 0.25;
    fill(
        (0..48)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / 48.0;
                centre + vec2(a.cos(), a.sin()) * radius
            })
            .collect(),
        c.accent,
    );
    rect((0.8, 0.39), (0.94, 0.61), c.panel);
}

#[derive(Clone, Copy)]
pub(crate) struct LayoutPreview {
    pub compact: bool,
    pub labels: bool,
    pub history: usize,
    pub translucent: bool,
}

/// One live shell specimen follows all four layout choices; it is a schematic, not a second scene.
pub(crate) fn layout_preview(ui: &mut Ui, state: LayoutPreview) {
    super::preferences_figures::paint_layout(ui, state);
}

pub(crate) fn tab_accent_strip(ui: &mut Ui) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    super::text(ui, crate::i18n::tr("prefs.tab_accent_note"), super::Text::Hint("tab_accent"));
    // Named as the ribbon names its tabs, so each reads as its tab does in every language.
    let names = ["MODEL", "SURFACE", "MESH", "FORM", "SHEET METAL", "RENDER"];
    let (r, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_24 + t.space_8), Sense::hover());
    let width = r.width() / names.len() as f32;
    for (i, name) in names.iter().enumerate() {
        let rect = Rect::from_min_size(r.min + vec2(i as f32 * width, 0.0), vec2((width - t.space_6).max(0.0), t.space_6));
        let accent = crate::theme::TabAccent::for_tab(&t, name);
        ui.painter().rect_filled(rect, t.radius_sm, accent.color);
        let label = Rect::from_min_size(pos2(rect.left(), rect.bottom() + t.space_4), vec2(rect.width(), t.space_16));
        centred_text(ui.painter(), label, &crate::workspace::tab_name(name), FontId::proportional(t.font_small), t.form_dim);
    }
}

pub(crate) fn scale_specimen(ui: &mut Ui, scale: f32) {
    let t = Tokens::get();
    let scale = if scale.is_finite() { scale.clamp(0.5, 2.5) / ui.ctx().zoom_factor().max(0.5) } else { 1.0 };
    let (r, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_24 * 4.0 + t.space_16), Sense::hover());
    t.container_rect(ui.painter(), r, t.radius_lg, t.control_fill, t.dialog_rule);
    let p = ui.painter().with_clip_rect(r.shrink(t.space_8));
    let font = FontId::proportional(t.font_label * scale);
    p.text(
        r.right_top() + vec2(-t.space_8, t.space_8),
        egui::Align2::RIGHT_TOP,
        crate::i18n::tr("prefs.live_preview").to_uppercase(),
        FontId::proportional(t.font_micro),
        t.form_dim,
    );
    let origin = r.min + vec2(t.space_10, t.space_24);
    let line_h = t.control_height * scale;
    let distance = crate::i18n::tr_label("Distance");
    let label = p.layout_no_wrap(distance.to_owned(), font.clone(), t.form_dim);
    p.galley(origin + vec2(0.0, (line_h - label.size().y) / 2.0), label.clone(), t.form_dim);
    let field = Rect::from_min_size(origin + vec2(label.size().x + t.space_8 * scale, 0.0), vec2((t.form_label_width + t.space_6) * scale, line_h));
    p.rect(field, t.control_radius, t.input_fill(), t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    if let Some(bevel) = t.borders.bevel {
        bevel.paint(&p, field, t.control_radius, true, true);
    }
    centred_text(&p, field, "12.50", FontId::monospace(t.font_body * scale), t.form_ink);
    let button = Rect::from_min_size(field.right_top() + vec2(t.space_8 * scale, 0.0), vec2(t.space_24 * 1.5 * scale, line_h));
    p.rect_filled(button, t.control_radius, if t.borders.bevel.is_some() { t.control_fill } else { t.form_primary });
    if let Some(bevel) = t.borders.bevel {
        bevel.paint(&p, button, t.control_radius, false, true);
    }
    centred_text(
        &p,
        button,
        crate::i18n::tr_label("OK"),
        crate::fonts::semibold(t.font_label * scale),
        if t.borders.bevel.is_some() { t.form_ink } else { t.on_accent },
    );
    let feature = Rect::from_min_size(origin + vec2(0.0, line_h + t.space_8 * scale), vec2(r.width() - t.space_10 * 2.0, line_h));
    p.rect_filled(feature, t.control_radius, t.selected_bg);
    let icon = Rect::from_center_size(pos2(feature.left() + t.space_16 * scale, feature.center().y), vec2(t.space_16, t.space_16) * scale);
    crate::icons::paint_on_selected(&p, icon, "extrude", 1.0);
    p.text(
        pos2(icon.right() + t.space_6 * scale, feature.center().y),
        egui::Align2::LEFT_CENTER,
        "Extrude1",
        crate::fonts::semibold(t.font_label * scale),
        t.selected_ink,
    );
    p.text(feature.right_center() - vec2(t.space_8, 0.0), egui::Align2::RIGHT_CENTER, "12.5 mm", font, t.selected_ink);
}

/// Font picker and auxiliary actions share one line, with the specimen below.
pub(crate) fn settings_font_row(ui: &mut Ui, label: &str, mut add: impl FnMut(&mut Ui, usize)) {
    let t = Tokens::get();
    super::settings_form(ui, t.form_label_width, |ui| {
        super::row(ui, label, |ui| {
            let actions = t.space_24 * 8.0 + t.space_8;
            let picker = (dimension(ui.available_width()) - actions - t.space_8).max(t.space_24);
            ui.allocate_ui_with_layout(vec2(picker, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| add(ui, 0));
            ui.add_space(t.space_4);
            add(ui, 1);
        });
    });
}

/// Natural-width dim text, for inline suffixes and metadata without a full-width note row.
pub(crate) fn settings_inline_note(ui: &mut Ui, text: &str) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(t.font_body), t.form_dim);
    let (r, response) = ui.allocate_exact_size(galley.size(), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), text));
    ui.painter().galley(r.min, galley, t.form_dim);
}

/// Framed font specimen, inheriting the chosen application font and its fallback glyphs.
pub(crate) fn font_specimen(ui: &mut Ui, text: &str) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let (r, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.control_height + t.space_10), Sense::hover());
    t.container_rect(ui.painter(), r, t.control_radius, t.control_fill, t.dialog_rule);
    paint_text(ui, r.shrink2(vec2(t.space_12, 0.0)), text, FontId::proportional(t.font_button), t.form_ink, false);
}

/// Intrinsically sized segments, retaining the usual row label for accessibility and automation.
pub(crate) fn settings_segments(ui: &mut Ui, label: &str, selected: &mut usize, options: &[&str]) -> bool {
    if crate::prefs_search::suppress_chrome(ui) && !crate::prefs_search::texts_hit(ui, &[label]) && !crate::prefs_search::texts_hit(ui, options) {
        return false;
    }
    let t = Tokens::get();
    let widest = options.iter().map(|s| super::ContentWidth::name(ui, s)).fold(0.0, f32::max);
    let width = ((widest + t.space_16) * options.len() as f32).min(dimension(ui.available_width()));
    ui.allocate_ui_with_layout(vec2(width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.scope_builder(
            egui::UiBuilder::new().ui_stack_info(egui::UiStackInfo::default().with_tag_value(super::layout::LABEL, label.to_owned())),
            |ui| super::segmented(ui, selected, options, super::Segments { compact: true, ..Default::default() }),
        )
        .inner
    })
    .inner
}

pub(crate) fn status_palette_sample(ui: &mut Ui, mode: ColorVision) {
    if crate::prefs_search::suppress_chrome(ui) {
        return;
    }
    let t = Tokens::get();
    let colours = Tokens::themed(crate::theme::is_dark(), mode);
    ui.horizontal_wrapped(|ui| {
        for (label, icon, fill) in [
            ("prefs.status_selected", "finish", colours.form_primary),
            ("prefs.status_warning", "warning", colours.warning),
            ("prefs.status_error", "close", colours.error),
            ("prefs.status_done", "finish", colours.form_success),
        ] {
            let ink = if crate::theme::contrast_ratio(t.on_accent, fill) >= 4.5 { t.on_accent } else { t.tab_on_fill_dark };
            let label = crate::i18n::tr(label);
            let galley = ui.painter().layout_no_wrap(label.to_owned(), crate::fonts::semibold(t.font_small), ink);
            let (r, _) = ui.allocate_exact_size(vec2(galley.size().x + t.space_24 + t.space_16, t.space_24), Sense::hover());
            ui.painter().rect_filled(r, t.radius_xl, fill);
            crate::icons::paint_ink(
                ui.painter(),
                Rect::from_center_size(pos2(r.left() + t.space_12, r.center().y), vec2(t.space_12, t.space_12)),
                icon,
                ink,
            );
            ui.painter().galley(pos2(r.left() + t.space_24, r.center().y - galley.size().y / 2.0), galley, ink);
        }
    });
}

/// The names under the Colour vision sample (message ids).
pub(crate) const COLOUR_VISION_SAMPLE: [&str; 6] =
    ["ui.colour_vision_fixed", "ui.colour_vision_projected", "ui.axes", "Bends", "ui.draft", "ui.colour_vision_failed"];

/// The Colour vision sample (#778): fixed and projected sketch lines, the X and Y axes, bends up
/// and down, Draft Analysis and a failed feature, each drawn as it is where it appears, in the
/// colours of `t` (the preference passes the mode being chosen, #7152). The six cells share one
/// responsive, clipped area.
pub(crate) fn colour_vision_sample(ui: &mut egui::Ui, t: &Tokens) {
    let width = (dimension(ui.available_width()) / COLOUR_VISION_SAMPLE.len() as f32).min(t.space_24 * 3.0);
    let line_width = t.space_4;
    let (area, _) = ui.allocate_exact_size(egui::vec2(width * COLOUR_VISION_SAMPLE.len() as f32, t.space_24 * 2.0 + t.space_2), egui::Sense::hover());
    let p = ui.painter_at(area);
    let line = |c| egui::Stroke::new(line_width, c);
    for (i, name) in COLOUR_VISION_SAMPLE.iter().enumerate() {
        let cell = egui::Rect::from_min_size(
            area.min + egui::vec2(width * i as f32, 0.0),
            egui::vec2((width - t.space_8).max(0.0), t.control_height + t.space_4),
        );
        let (a, b) = (cell.left_center() + egui::vec2(t.space_8, 0.0), cell.right_center() - egui::vec2(t.space_8, 0.0));
        match i {
            0 | 1 => {
                p.rect_filled(cell, t.radius_sm, t.viewport_top);
                p.line_segment([a, b], line(if i == 0 { t.sketch_locked } else { t.sketch_projected }));
            }
            2 => {
                // X along and Y up from one corner, as the view triad draws them.
                p.rect_filled(cell, t.radius_sm, t.viewport_top);
                let o = cell.left_bottom() + egui::vec2(t.space_12, -(t.space_4 + t.space_3));
                p.line_segment([o, egui::pos2(cell.right() - t.space_10, o.y)], line(t.axis_x));
                p.line_segment([o, egui::pos2(o.x, cell.top() + t.space_6)], line(t.axis_y));
            }
            3 => {
                // Dashed, as the flat pattern draws them.
                p.rect_filled(cell, t.radius_sm, t.viewport_top);
                for (dy, c) in [(-(t.space_4 + t.space_3), t.bend_up), (t.space_4 + t.space_3, t.bend_down)] {
                    p.extend(egui::Shape::dashed_line(
                        &[a + egui::vec2(0.0, dy), b + egui::vec2(0.0, dy)],
                        line(c),
                        t.space_8,
                        t.space_3 + t.space_2,
                    ));
                }
            }
            4 => {
                // The three bands of Draft Analysis; in a mode the negative one is striped.
                let w = cell.width() / 3.0;
                for (k, c) in t.analysis.draft.iter().enumerate() {
                    let band = egui::Rect::from_min_size(cell.min + egui::vec2(w * k as f32, 0.0), egui::vec2(w, cell.height()));
                    p.rect_filled(band, 0.0, crate::theme::srgb(*c));
                    if k == 2 && t.analysis.hatch {
                        let stripes = p.with_clip_rect(band);
                        let dark = crate::theme::srgb(c.map(|x| x * 0.6));
                        let mut x = band.left() - band.height();
                        while x < band.right() {
                            stripes.line_segment(
                                [egui::pos2(x, band.bottom()), egui::pos2(x + band.height(), band.top())],
                                egui::Stroke::new(2.0, dark),
                            );
                            x += t.space_6 + t.space_3;
                        }
                    }
                }
            }
            _ => {
                // A failed feature on the timeline.
                p.rect(
                    cell.shrink2(egui::vec2(t.space_10, t.space_3)),
                    t.radius_sm,
                    t.timeline_item_error,
                    egui::Stroke::new(1.0, t.error),
                    egui::StrokeKind::Inside,
                );
            }
        }
        centred_text(
            &p,
            Rect::from_min_size(cell.left_bottom() + vec2(0.0, t.space_3), vec2(cell.width(), t.space_16)),
            crate::i18n::tr(name),
            FontId::proportional(t.font_small),
            t.text_dim,
        );
    }
}
