//! Preference compositions for the approved Navigation bar board.
use super::{Container, Surface, navigation::*};
use crate::icons;
use crate::ui_kit::BackingTooltipExt as _;
use egui::{FontId, Rect, Response, Sense, Ui, pos2, vec2};
use solidcraft_theme::Tokens;

pub(crate) const NAV_STAGE_HEIGHT: f32 = 96.0;
pub(crate) const NAV_STAGE_GRID: f32 = 16.0;
const NAV_ANCHOR_INSET: f32 = 9.0;
pub(crate) const NAV_STAGE_INSET: f32 = 12.0;
pub(crate) const NAV_STAGE_RADIUS: u8 = 8;
pub(crate) const NAV_TILE_HEIGHT: f32 = 30.0 + 2.0 * NAV_BORDER;
pub(crate) const NAV_TILE_ICON: f32 = 16.0;
pub(crate) const NAV_TILE_ADD: f32 = 22.0;
pub(crate) const NAV_MARKER_HEIGHT: f32 = 24.0;
pub(crate) const NAV_GHOST_OPACITY: f32 = 0.7;
pub(crate) const NAV_TILE_DRAG_OPACITY: f32 = 0.55;
const NAV_TILE_INSET: f32 = 9.0;
const NAV_TILE_GAP: f32 = 7.0;
const NAV_TILE_RADIUS: u8 = 7;

/// The preview stage is a miniature of the 3D view: its background gradient and ground-grid colours.
pub(crate) fn navigation_stage(ui: &mut Ui, width: f32) -> Rect {
    let t = Tokens::get();
    let view = Tokens::view();
    let (rect, _) = ui.allocate_exact_size(vec2(width.max(0.0), NAV_STAGE_HEIGHT), Sense::hover());
    let inner = rect.shrink(NAV_BORDER);
    let outline = super::preferences::rounded_perimeter(inner, f32::from(NAV_STAGE_RADIUS));
    paint_view_backdrop(ui.painter(), inner, &outline, &view);
    paint_view_grid(ui.painter(), inner, f32::from(NAV_STAGE_RADIUS), &view);
    ui.painter().add(egui::Shape::closed_line(outline, egui::Stroke::new(NAV_BORDER, t.dialog_border)));
    rect
}

/// Fills the rounded outline with the view's vertical gradient (a fan from the centre).
fn paint_view_backdrop(painter: &egui::Painter, rect: Rect, outline: &[egui::Pos2], view: &Tokens) {
    if outline.len() < 3 || rect.height() <= 0.0 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), view.viewport_top.lerp_to_gamma(view.viewport_bottom, 0.5));
    for point in outline {
        let along = ((point.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        mesh.colored_vertex(*point, view.viewport_top.lerp_to_gamma(view.viewport_bottom, along));
    }
    let count = outline.len() as u32;
    for index in 0..count {
        mesh.add_triangle(0, 1 + index, 1 + (index + 1) % count);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// How far a grid line must stop short of the top or bottom for the rounded corner.
fn corner_inset(distance_from_side: f32, radius: f32) -> f32 {
    if radius <= 0.0 || distance_from_side >= radius {
        return 0.0;
    }
    let across = (radius - distance_from_side.max(0.0)).min(radius);
    radius - (radius * radius - across * across).max(0.0).sqrt()
}

/// The viewport's grid, flat: minor lines every [`NAV_STAGE_GRID`], a major line every fourth.
fn paint_view_grid(painter: &egui::Painter, rect: Rect, radius: f32, view: &Tokens) {
    let centre = rect.center();
    let lines = |origin: f32, low: f32, high: f32| -> Vec<(f32, bool)> {
        let first = ((low - origin) / NAV_STAGE_GRID).ceil() as i32;
        let last = ((high - origin) / NAV_STAGE_GRID).floor() as i32;
        (first..=last).map(|k| (origin + k as f32 * NAV_STAGE_GRID, k % 4 == 0)).collect()
    };
    let stroke = |major: bool| egui::Stroke::new(if major { 1.0 } else { 0.7 }, if major { view.grid_major } else { view.grid });
    for (x, major) in lines(centre.x, rect.left() + 1.0, rect.right() - 1.0) {
        let inset = corner_inset((x - rect.left()).min(rect.right() - x), radius);
        painter.vline(x, (rect.top() + inset)..=(rect.bottom() - inset), stroke(major));
    }
    for (y, major) in lines(centre.y, rect.top() + 1.0, rect.bottom() - 1.0) {
        let inset = corner_inset((y - rect.top()).min(rect.bottom() - y), radius);
        painter.hline((rect.left() + inset)..=(rect.right() - inset), y, stroke(major));
    }
}

/// The anchor field's miniature, measured from the shared spacing tokens.
pub(crate) fn navigation_anchor_size() -> egui::Vec2 {
    let t = Tokens::get();
    vec2(t.space_24 * 5.0 + t.space_4, t.space_24 * 3.0)
}

/// Six anchors (left, centre, right on the top and bottom edge) on a miniature of the view.
/// Returns the clicked anchor, row-major: 0-2 along the top, 3-5 along the bottom.
pub(crate) fn navigation_anchor_picker(ui: &mut Ui, id: &str, selected: usize, labels: &[&str; 6]) -> Option<usize> {
    let t = Tokens::get();
    let view = Tokens::view();
    let (rect, _) = ui.allocate_exact_size(navigation_anchor_size(), Sense::hover());
    let outline = super::preferences::rounded_perimeter(rect.shrink(NAV_BORDER), f32::from(t.control_radius));
    paint_view_backdrop(ui.painter(), rect.shrink(NAV_BORDER), &outline, &view);
    ui.painter().add(egui::Shape::closed_line(outline, egui::Stroke::new(NAV_BORDER, t.control_border)));
    let pill_size = vec2(t.space_24 + t.space_6, t.space_10);
    let mut clicked = None;
    for (index, label) in labels.iter().enumerate() {
        let (row, column) = (index / 3, index % 3);
        let x = match column {
            0 => rect.left() + NAV_ANCHOR_INSET + pill_size.x / 2.0,
            1 => rect.center().x,
            _ => rect.right() - NAV_ANCHOR_INSET - pill_size.x / 2.0,
        };
        let y = if row == 0 { rect.top() + NAV_ANCHOR_INSET + pill_size.y / 2.0 } else { rect.bottom() - NAV_ANCHOR_INSET - pill_size.y / 2.0 };
        let pill = Rect::from_center_size(pos2(x, y), pill_size);
        // The hit area is the whole third of the miniature, so the target is easy to reach.
        let cell = Rect::from_min_max(
            pos2(rect.left() + rect.width() * column as f32 / 3.0, rect.top() + rect.height() * row as f32 / 2.0),
            pos2(rect.left() + rect.width() * (column + 1) as f32 / 3.0, rect.top() + rect.height() * (row + 1) as f32 / 2.0),
        );
        let response = ui.interact(cell, ui.scope_id().with((id, "anchor", index)), Sense::click());
        let on = index == selected;
        let (fill, stroke) = if on {
            (t.form_primary, egui::Stroke::new(NAV_BORDER, t.form_primary))
        } else if response.hovered() {
            (t.nav_hover, egui::Stroke::new(NAV_BORDER, t.form_primary))
        } else {
            (t.nav_field, egui::Stroke::new(NAV_BORDER, t.control_border))
        };
        ui.painter().rect(pill, 4, fill, stroke, egui::StrokeKind::Inside);
        if response.has_focus() {
            ui.painter().rect_stroke(
                pill.expand(NAV_FOCUS_WIDTH + 1.0),
                5,
                egui::Stroke::new(NAV_FOCUS_WIDTH, t.form_primary),
                egui::StrokeKind::Outside,
            );
        }
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), on, *label));
        solidcraft_ui_registry::publish_target(&format!("prefs:navbar:anchor:{index}"), cell);
        if response.backing_on_hover_text(*label).clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

/// A labelled row holding the anchor picker, shaped like the other Navigation bar options.
pub(crate) fn settings_anchor_field(ui: &mut Ui, label: &str, selected: usize, names: &[&str; 6]) -> Option<usize> {
    let t = Tokens::get();
    super::settings_form(ui, 0.0, |ui| {
        super::layout::sized_row(ui, label, navigation_anchor_size().y + t.space_16 + t.space_4, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = t.space_4;
                super::settings_inline_note(ui, label);
                navigation_anchor_picker(ui, "anchor", selected, names)
            })
            .inner
        })
    })
}

/// The source of separators: a dashed, label-only chip, apart from the tiles of functional buttons.
pub(crate) fn navigation_separator_source(ui: &mut Ui, id: &str, label: &str, dragging: bool) -> (Response, Response) {
    let t = Tokens::get();
    let text = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(NAV_TEXT), t.form_dim);
    let width = (2.0 * NAV_BORDER + NAV_TILE_INSET + NAV_TILE_ICON + NAV_TILE_GAP + text.size().x + NAV_TILE_GAP + NAV_TILE_ADD + t.space_4)
        .min(ui.max_rect().width().max(0.0));
    let (rect, _) = ui.allocate_exact_size(vec2(width, NAV_TILE_HEIGHT), Sense::hover());
    let response = ui.interact(rect, ui.scope_id().with((id, "tile")), Sense::click_and_drag());
    let add_rect =
        Rect::from_center_size(pos2(rect.right() - NAV_BORDER - t.space_4 - NAV_TILE_ADD / 2.0, rect.center().y), vec2(NAV_TILE_ADD, NAV_TILE_ADD));
    let add = ui.interact(add_rect, ui.scope_id().with((id, "add")), Sense::click());
    let mut painter = ui.painter().clone();
    if dragging {
        painter.multiply_opacity(NAV_TILE_DRAG_OPACITY);
    }
    if response.hovered() {
        painter.rect_filled(rect, NAV_TILE_RADIUS, t.nav_hover);
    }
    let mut outline = super::preferences::rounded_perimeter(rect.shrink(NAV_BORDER / 2.0), f32::from(NAV_TILE_RADIUS));
    if let Some(first) = outline.first().copied() {
        outline.push(first);
    }
    painter.extend(egui::Shape::dashed_line(&outline, egui::Stroke::new(NAV_BORDER, t.control_border), t.space_4, t.space_3));
    // The glyph is the bar's own divider, flanked by the buttons it separates.
    let glyph = Rect::from_center_size(
        pos2(rect.left() + NAV_BORDER + NAV_TILE_INSET + NAV_TILE_ICON / 2.0, rect.center().y),
        vec2(NAV_TILE_ICON, NAV_TILE_ICON),
    );
    for dx in [-5.0, 5.0] {
        painter.rect_filled(Rect::from_center_size(pos2(glyph.center().x + dx, glyph.center().y), vec2(t.space_3, t.space_3)), 1, t.form_dim);
    }
    painter.vline(glyph.center().x, glyph.y_range(), egui::Stroke::new(NAV_BORDER * 1.5, t.form_dim));
    painter
        .with_clip_rect(rect.intersect(ui.clip_rect()).intersect(Rect::from_min_max(rect.min, pos2(add_rect.left() - NAV_TILE_GAP, rect.bottom()))))
        .galley(pos2(glyph.right() + NAV_TILE_GAP, rect.center().y - text.size().y / 2.0), text, t.form_dim);
    if add.hovered() {
        painter.rect_filled(add_rect, t.control_radius.saturating_sub(NAV_BORDER as u8), t.nav_hover);
    }
    icons::paint_ink(&painter, Rect::from_center_size(add_rect.center(), vec2(t.space_14, t.space_14)), "plus", t.form_primary);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    add.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), solidcraft_ui_text::format("navbar.custom.add_slot", &[("slot", label)]))
    });
    (response, add)
}

pub(crate) fn navigation_tile(ui: &mut Ui, id: &str, label: &str, icon: &str, dragging: bool) -> (Response, Response) {
    let t = Tokens::get();
    let text = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(NAV_TEXT), t.form_ink);
    let width = (2.0 * NAV_BORDER + NAV_TILE_INSET + NAV_TILE_ICON + NAV_TILE_GAP + text.size().x + NAV_TILE_GAP + NAV_TILE_ADD + t.space_4)
        .min(ui.max_rect().width().max(0.0));
    let (rect, _) = ui.allocate_exact_size(vec2(width, NAV_TILE_HEIGHT), Sense::hover());
    let response = ui.interact(rect, ui.scope_id().with((id, "tile")), Sense::click_and_drag());
    let add_rect =
        Rect::from_center_size(pos2(rect.right() - NAV_BORDER - t.space_4 - NAV_TILE_ADD / 2.0, rect.center().y), vec2(NAV_TILE_ADD, NAV_TILE_ADD));
    let add = ui.interact(add_rect, ui.scope_id().with((id, "add")), Sense::click());
    let mut painter = ui.painter().clone();
    if dragging {
        painter.multiply_opacity(NAV_TILE_DRAG_OPACITY);
    }
    painter.rect(
        rect,
        NAV_TILE_RADIUS,
        if response.hovered() { t.nav_hover } else { t.nav_field },
        egui::Stroke::new(NAV_BORDER, t.control_border),
        egui::StrokeKind::Inside,
    );
    let icon_rect = Rect::from_center_size(
        pos2(rect.left() + NAV_BORDER + NAV_TILE_INSET + NAV_TILE_ICON / 2.0, rect.center().y),
        vec2(NAV_TILE_ICON, NAV_TILE_ICON),
    );
    if icon.is_empty() {
        painter.vline(icon_rect.center().x, icon_rect.y_range(), egui::Stroke::new(NAV_BORDER, t.form_dim));
    } else {
        icons::paint_ink(&painter, icon_rect, icon, t.form_dim);
    }
    painter
        .with_clip_rect(rect.intersect(ui.clip_rect()).intersect(Rect::from_min_max(rect.min, pos2(add_rect.left() - NAV_TILE_GAP, rect.bottom()))))
        .galley(pos2(icon_rect.right() + NAV_TILE_GAP, rect.center().y - text.size().y / 2.0), text, t.form_ink);
    if add.hovered() {
        painter.rect_filled(add_rect, t.control_radius.saturating_sub(NAV_BORDER as u8), t.nav_hover);
    }
    icons::paint_ink(&painter, Rect::from_center_size(add_rect.center(), vec2(t.space_14, t.space_14)), "plus", t.form_primary);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    add.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), solidcraft_ui_text::format("navbar.custom.add_slot", &[("slot", label)]))
    });
    (response, add)
}

pub(crate) fn navigation_drag_ghost(ui: &Ui, at: egui::Pos2, button: NavigationButton<'_>, size: NavigationSize) {
    let ctx = ui.ctx();
    let t = Tokens::get();
    let metrics = NavigationMetrics::for_size(size);
    let width =
        if button.menu { metrics.icon + NAV_MENU_LEFT + NAV_MENU_RIGHT + NAV_MENU_GAP + NAV_CHEVRON + 2.0 * NAV_BORDER } else { metrics.button };
    Container::new(ctx, Surface::Navigation)
        .area(ui.scope_id().with("navigation_drag_ghost"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(at + vec2(t.space_14, t.space_16))
        .show(ctx, |ui| {
            ui.set_opacity(NAV_GHOST_OPACITY);
            let (rect, _) = ui.allocate_exact_size(vec2(width, metrics.button), Sense::hover());
            ui.painter().rect_filled(rect, metrics.button_radius, t.nav_hover);
            super::navigation_button_editable(ui, rect, button, true);
        });
}
