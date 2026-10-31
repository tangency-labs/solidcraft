//! Preference compositions for the approved Navigation bar board.
use super::{Container, Surface, navigation::*};
use crate::{icons, theme::Tokens};
use egui::{FontId, Rect, Response, Sense, Ui, pos2, vec2};

pub(crate) const NAV_STAGE_HEIGHT: f32 = 58.0;
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

pub(crate) fn navigation_stage(ui: &mut Ui, width: f32) -> Rect {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(width.max(0.0), NAV_STAGE_HEIGHT), Sense::hover());
    let frame = Container::new(ui.ctx(), Surface::Canvas)
        .frame(ui.ctx(), t.nav_viewport, crate::frame::Side::Left)
        .stroke(egui::Stroke::new(NAV_BORDER, t.dialog_border))
        .corner_radius(NAV_STAGE_RADIUS);
    Container::paint_frame(ui.painter(), frame, rect.shrink(NAV_BORDER));
    rect
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
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), crate::i18n::format("navbar.custom.add_slot", &[("slot", label)]))
    });
    (response, add)
}

pub(crate) fn navigation_drag_ghost(ui: &Ui, at: egui::Pos2, button: NavigationButton<'_>, size: crate::navigation_bar::Size) {
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
