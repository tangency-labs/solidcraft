//! Popup and tooltip skin from the approved navigation boards.
use super::{Container, Surface, navigation::*};
use egui::{Align2, Rect, Response, pos2};
use solidcraft_theme::Tokens;

pub fn navigation_popup_frame() -> egui::Frame {
    crate::Menu::frame()
}

/// A tip beyond `along` (the bar, or the row it names): above it, or below it for a bar on the
/// view's top edge.
pub fn navigation_tooltip(response: &Response, along: Rect, edge: NavigationEdge, label: &str, shortcut: Option<&str>) {
    let ctx = &response.ctx;
    let id = response.id.with("nav_tooltip_delay");
    if !response.hovered() {
        ctx.data_mut(|data| data.remove::<f64>(id));
        return;
    }
    let now = ctx.input(|i| i.time);
    let began = ctx.data_mut(|data| *data.get_temp_mut_or_insert_with(id, || now));
    if now - began < NAV_TOOLTIP_DELAY {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((NAV_TOOLTIP_DELAY - (now - began)).clamp(0.0, NAV_TOOLTIP_DELAY)));
        return;
    }
    let t = Tokens::get();
    let frame =
        egui::Frame::NONE.fill(t.nav_tip).corner_radius(t.control_radius).inner_margin(egui::Margin::symmetric(t.space_8 as i8, NAV_POPUP_PADDING));
    Container::new(ctx, Surface::Popup)
        .area(response.id.with("nav_tooltip"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .pivot(if edge == NavigationEdge::Top { Align2::CENTER_TOP } else { Align2::CENTER_BOTTOM })
        .fixed_pos(match edge {
            NavigationEdge::Bottom => pos2(response.rect.center().x, along.top() - NAV_POPUP_GAP),
            NavigationEdge::Top => pos2(response.rect.center().x, along.bottom() + NAV_POPUP_GAP),
        })
        .show(ctx, |ui| {
            Container::new(ctx, Surface::Popup).show_frame(ui, frame, ui.max_rect(), |ui| {
                ui.spacing_mut().item_spacing.x = t.space_8;
                ui.spacing_mut().interact_size.y = 0.0;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(label).font(crate::type_face::medium(t.font_body)).color(t.nav_tip_ink));
                    if let Some(key) = shortcut {
                        egui::Frame::NONE
                            .stroke(egui::Stroke::new(NAV_BORDER, t.nav_tip_key_border))
                            .corner_radius(t.radius_sm)
                            .inner_margin(egui::Margin::symmetric(NAV_POPUP_PADDING, 0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(key)
                                        .monospace()
                                        .size(t.font_small)
                                        .line_height(Some(NAV_TOOLTIP_KEY_LINE_HEIGHT))
                                        .color(t.nav_tip_ink),
                                );
                            });
                    }
                });
            });
        });
}
