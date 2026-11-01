//! Popup and tooltip skin from the approved navigation boards.
use super::{Container, Surface, navigation::*};
use crate::ui_kit::BackingTooltipExt as _;
use crate::{icons, navigation_bar::Edge, theme::Tokens};
use egui::{Align2, FontId, Rect, Response, Sense, Ui, pos2, vec2};

pub(crate) fn navigation_popup_frame() -> egui::Frame {
    Container::menu_frame().inner_margin(egui::Margin::same(NAV_POPUP_PADDING))
}

pub(crate) fn navigation_menu<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    Container::new(ui.ctx(), Surface::Popup)
        .show_frame(ui, navigation_popup_frame(), ui.max_rect(), |ui| {
            ui.set_width((width - 2.0 * (f32::from(NAV_POPUP_PADDING) + NAV_BORDER)).max(0.0));
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), add).inner
        })
        .inner
}

pub(crate) fn navigation_context<R>(response: &Response, add: impl FnOnce(&mut Ui) -> R) {
    Container::new(&response.ctx, Surface::Popup).context_menu(response).frame(navigation_popup_frame()).show(|ui| {
        ui.set_width(NAV_CONTEXT_WIDTH - 2.0 * (f32::from(NAV_POPUP_PADDING) + NAV_BORDER));
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui)
    });
}

pub(crate) struct NavigationMenuRow<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub icon: &'a str,
    pub selected: bool,
    pub enabled: bool,
    pub detail: Option<&'a str>,
    pub submenu: bool,
}
impl<'a> NavigationMenuRow<'a> {
    pub(crate) fn new(id: &'a str, label: &'a str, icon: &'a str) -> Self {
        Self { id, label, icon, selected: false, enabled: true, detail: None, submenu: false }
    }
    pub(crate) fn show(self, ui: &mut Ui) -> Response {
        let t = Tokens::get();
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), NAV_MENU_ROW), Sense::hover());
        let response = ui.interact(rect, ui.id().with(self.id), if self.enabled { Sense::click() } else { Sense::hover() });
        let ink = if self.selected { t.form_primary } else { t.form_ink };
        let bg = if self.selected {
            Some(t.selected_bg)
        } else if response.hovered() && self.enabled {
            Some(t.nav_hover)
        } else {
            None
        };
        if let Some(bg) = bg {
            ui.painter().rect_filled(rect, NAV_MENU_RADIUS, bg);
        }
        if !self.icon.is_empty() {
            let icon =
                Rect::from_center_size(pos2(rect.left() + NAV_MENU_INSET + NAV_MENU_ICON / 2.0, rect.center().y), vec2(NAV_MENU_ICON, NAV_MENU_ICON));
            icons::paint_ink(ui.painter(), icon, self.icon, if self.selected { ink } else { t.form_dim });
        }
        let font = if self.selected { crate::fonts::semibold(NAV_TEXT) } else { FontId::proportional(NAV_TEXT) };
        // The label ends before the detail and the check or chevron: one that doesn't fit is cut
        // with an ellipsis and shown whole on hover, as a long name in a list is (#8700).
        let mut end = rect.right() - NAV_MENU_INSET - if self.selected || self.submenu { t.space_14 + t.space_4 } else { 0.0 };
        if let Some(detail) = self.detail {
            let at = rect.right() - NAV_MENU_INSET - if self.submenu { NAV_CHEVRON + t.space_4 } else { 0.0 };
            let detail = ui.painter().text(
                pos2(at, rect.center().y),
                Align2::RIGHT_CENTER,
                detail,
                FontId::proportional(if self.enabled { t.font_body } else { t.font_small }),
                if self.enabled { t.form_dim } else { t.nav_dim },
            );
            end = end.min(detail.left() - t.space_8);
        }
        let left = rect.left() + NAV_MENU_INSET + NAV_MENU_ICON + NAV_ROW_GAP;
        let mut job = egui::text::LayoutJob::simple_singleline(self.label.to_owned(), font, ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width((end - left).max(0.0));
        let galley = ui.painter().layout_job(job);
        let cut = galley.elided;
        ui.painter().galley(pos2(left, rect.center().y - galley.size().y / 2.0), galley, ink);
        let tail = Rect::from_center_size(pos2(rect.right() - NAV_MENU_INSET - t.space_14 / 2.0, rect.center().y), vec2(t.space_14, t.space_14));
        if self.selected {
            super::checks::paint_check_glyph(ui.painter(), tail, false, t.form_primary);
        } else if self.submenu {
            let arrow =
                Rect::from_center_size(pos2(rect.right() - NAV_MENU_INSET - NAV_CHEVRON / 2.0, rect.center().y), vec2(NAV_CHEVRON, NAV_CHEVRON));
            icons::paint_ink(ui.painter(), arrow, "chevron", t.form_dim);
        }
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, self.enabled && ui.is_enabled(), self.selected, self.label));
        if cut { response.backing_on_hover_text(self.label) } else { response }
    }
}

pub(crate) fn navigation_menu_heading(ui: &mut Ui, title: &str) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), NAV_MENU_HEADING), Sense::hover());
    let mut format = egui::TextFormat::simple(crate::fonts::semibold(NAV_HEADING_TEXT), t.form_dim);
    format.extra_letter_spacing = NAV_HEADING_TEXT * NAV_HEADING_TRACKING;
    let galley = ui.painter().layout_job(egui::text::LayoutJob::single_section(title.to_uppercase(), format));
    ui.painter().galley(rect.min + vec2(t.space_8, t.space_6), galley, t.form_dim);
}

pub(crate) fn navigation_menu_separator(ui: &mut Ui) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(0.0), NAV_BORDER + 2.0 * t.space_4), Sense::hover());
    ui.painter().hline(rect.shrink2(vec2(t.space_2, 0.0)).x_range(), rect.center().y, egui::Stroke::new(NAV_BORDER, t.dialog_rule));
}

/// A tip beyond `along` (the bar, or the row it names): above it, or below it for a bar on the
/// view's top edge.
pub(crate) fn navigation_tooltip(response: &Response, along: Rect, edge: Edge, label: &str, shortcut: Option<&str>) {
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
        .pivot(if edge == Edge::Top { Align2::CENTER_TOP } else { Align2::CENTER_BOTTOM })
        .fixed_pos(match edge {
            Edge::Bottom => pos2(response.rect.center().x, along.top() - NAV_POPUP_GAP),
            Edge::Top => pos2(response.rect.center().x, along.bottom() + NAV_POPUP_GAP),
        })
        .show(ctx, |ui| {
            Container::new(ctx, Surface::Popup).show_frame(ui, frame, ui.max_rect(), |ui| {
                ui.spacing_mut().item_spacing.x = t.space_8;
                ui.spacing_mut().interact_size.y = 0.0;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(label).font(crate::fonts::medium(t.font_body)).color(t.nav_tip_ink));
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
