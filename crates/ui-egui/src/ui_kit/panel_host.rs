//! Bounded universal workspace chrome. Placement and transactions are owned by
//! panel_runtime; every outer surface here uses the common Container policy.
use super::{Container, Surface};
use solidcraft_theme::Tokens;

pub(crate) struct HostChrome<'a> {
    pub label: &'a str,
    pub icon: Option<&'a str>,
    pub icon_only: bool,
    pub attention: bool,
    pub selected: bool,
    pub enabled: bool,
    pub drag: bool,
    pub title: bool,
    /// A tab group's header target paints only its section grip, retaining its label for accessibility.
    pub grip_only: bool,
    pub rtl: bool,
}

fn panel_host_container(ctx: &egui::Context, floating: bool) -> Container {
    if floating { Container::floating_card(Surface::Panel) } else { Container::new(ctx, Surface::Panel) }
}

/// The same adaptive frame supplies both paint and the content's safe clip.
pub(crate) fn panel_host_frame(ctx: &egui::Context, floating: bool) -> egui::Frame {
    panel_host_container(ctx, floating).frame(ctx, Tokens::get().panel, crate::frame::Side::Left).inner_margin(0).outer_margin(0)
}

pub(crate) fn panel_host_surface<R>(
    ui: &mut egui::Ui,
    id: egui::Id,
    bounds: egui::Rect,
    floating: bool,
    fill: egui::Color32,
    palette: Option<&Tokens>,
    body: impl FnOnce(&mut egui::Ui, egui::Rect) -> R,
) -> R {
    let _tokens = palette.map(|tokens| solidcraft_theme::scoped_panel_tokens(*tokens));
    let container = panel_host_container(ui.ctx(), floating);
    let mut frame = panel_host_frame(ui.ctx(), floating);
    frame.fill = fill;
    let clip = ui.clip_rect().intersect(bounds);
    // egui clips to rectangles. This inscribed rectangle keeps arbitrary content
    // fills inside the rounded frame without changing any header hit target.
    let radius = frame.corner_radius;
    let radius = f32::from(radius.nw.max(radius.ne).max(radius.sw).max(radius.se));
    let inset = frame.stroke.width + (radius - frame.stroke.width).max(0.0) * (1.0 - std::f32::consts::FRAC_1_SQRT_2);
    let content_clip = bounds.shrink(inset.min(bounds.width().min(bounds.height()).max(0.0) / 2.0)).intersect(clip);
    let paint = |ui: &mut egui::Ui| {
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(bounds));
        child.set_clip_rect(clip);
        container.paint_background(child.painter(), frame, bounds - frame.total_margin());
        if let Some(tokens) = palette {
            let visuals = child.visuals_mut();
            visuals.override_text_color = Some(tokens.text);
            visuals.weak_text_color = Some(tokens.form_dim);
            visuals.widgets.noninteractive.fg_stroke.color = tokens.text;
            for state in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.open] {
                state.fg_stroke.color = tokens.form_ink;
            }
        }
        let result = body(&mut child, content_clip);
        Container::paint_inset_clipped(child.painter(), bounds, frame, clip);
        result
    };
    if floating {
        let layer = egui::LayerId::new(egui::Order::Background, id.with("area"));
        Container::bring_to_front(ui.ctx(), layer);
        container
            .area(id.with("area"))
            .fixed_pos(bounds.min)
            .movable(false)
            .fade_in(false)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(bounds.size());
                paint(ui)
            })
            .inner
    } else {
        paint(ui)
    }
}

/// Actual accessible egui targets, with IDs independent of ordering and labels.
pub(crate) fn panel_host_control(ui: &mut egui::Ui, id: egui::Id, bounds: egui::Rect, chrome: HostChrome<'_>) -> egui::Response {
    panel_host_control_label_opacity(ui, id, bounds, chrome, 1.0)
}

/// Only the label ink changes; the grip, icons, fills and response stay intact.
pub(crate) fn panel_host_control_label_opacity(
    ui: &mut egui::Ui,
    id: egui::Id,
    bounds: egui::Rect,
    chrome: HostChrome<'_>,
    label_opacity: f32,
) -> egui::Response {
    let t = Tokens::get();
    let enabled = chrome.enabled && ui.is_enabled();
    let sense = if enabled { if chrome.drag { egui::Sense::click_and_drag() } else { egui::Sense::click() } } else { egui::Sense::hover() };
    let response = ui.interact(bounds, id, sense);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, chrome.label));
    let fill = if chrome.selected {
        Some(t.selected_bg)
    } else if response.hovered() && chrome.enabled {
        Some(t.hover)
    } else {
        None
    };
    let ink = if !chrome.enabled {
        t.form_dim
    } else if chrome.attention {
        t.form_primary
    } else {
        t.form_ink
    };
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(bounds));
    let paint_bounds = bounds.shrink2(egui::vec2(t.space_4.min(bounds.width() / 2.0), t.space_4.min(bounds.height() / 2.0)));
    if let Some(fill) = fill {
        if chrome.title {
            super::panel::panel_header_hover(&painter, paint_bounds);
        } else {
            painter.rect_filled(paint_bounds, t.control_radius, fill);
        }
    }
    if response.has_focus() {
        painter.rect_stroke(bounds.shrink(t.space_2), t.radius_sm, t.control_border_stroke(t.form_primary), egui::StrokeKind::Inside);
    }
    let text_inset = if chrome.title { t.space_12 } else { t.space_4 };
    let mut text = bounds.shrink2(egui::vec2(text_inset.min(bounds.width() / 2.0), 0.0));
    if chrome.title {
        let size =
            egui::vec2(t.space_10.min(if chrome.grip_only { bounds.width() } else { text.width() }), (t.space_16 + t.space_2).min(bounds.height()));
        let center = if chrome.grip_only {
            bounds.center()
        } else {
            egui::pos2(if chrome.rtl { text.right() - size.x / 2.0 } else { text.left() + size.x / 2.0 }, bounds.center().y)
        };
        let grip = egui::Rect::from_center_size(center, size);
        let mut grip_ui = ui.new_child(egui::UiBuilder::new().max_rect(grip));
        grip_ui.set_clip_rect(grip.intersect(ui.clip_rect()));
        super::panel_grip(&mut grip_ui);
        if chrome.rtl {
            text.max.x = (grip.left() - t.space_8).max(text.left());
        } else {
            text.min.x = (grip.right() + t.space_8).min(text.right());
        }
    }
    let icon = chrome.icon.filter(|icon| !icon.is_empty());
    if let Some(icon) = icon {
        let extent = if chrome.icon_only {
            (bounds.width().min(bounds.height()) - t.form_icon_inset * 2.0).max(0.0)
        } else {
            t.form_icon_size.min(bounds.height()).min(text.width())
        };
        let center = if chrome.icon_only {
            bounds.center()
        } else {
            egui::pos2(if chrome.rtl { text.right() - extent / 2.0 } else { text.left() + extent / 2.0 }, bounds.center().y)
        };
        let icon_rect = egui::Rect::from_center_size(center, egui::vec2(extent, extent));
        if chrome.icon_only {
            if let Some(transport) = icon.strip_prefix("timeline_").filter(|name| matches!(*name, "first" | "back" | "play" | "forward" | "last")) {
                let glyph = if transport == "play" && chrome.selected { "pause" } else { transport };
                crate::icons::glyphs::transport_glyph(&painter, icon_rect.center(), glyph, ink);
            } else {
                crate::icons::paint_ink(&painter, icon_rect, icon, ink);
            }
        } else {
            crate::icons::paint_icon(&painter, icon_rect, icon, if chrome.enabled { 1.0 } else { 0.5 });
        }
        let gap = if chrome.title { t.space_8 } else { t.space_4 };
        if chrome.rtl {
            text.max.x = (icon_rect.left() - gap).max(text.left());
        } else {
            text.min.x = (icon_rect.right() + gap).min(text.right());
        }
    }
    if chrome.attention && !chrome.grip_only {
        painter.circle_filled(
            egui::pos2(if chrome.rtl { paint_bounds.left() } else { paint_bounds.right() }, paint_bounds.top()),
            t.space_2,
            t.form_primary,
        );
    }
    if !chrome.icon_only && !chrome.grip_only {
        let ink = ink.gamma_multiply(label_opacity);
        let font = if chrome.title {
            crate::fonts::semibold(t.font_heading)
        } else if chrome.selected {
            crate::fonts::semibold(t.font_label)
        } else {
            egui::FontId::proportional(t.font_label)
        };
        if chrome.rtl {
            let width = painter.layout_no_wrap(chrome.label.to_owned(), font.clone(), ink).size().x.min(text.width());
            text.min.x = text.right() - width;
        }
        super::text::paint_text(ui, text, chrome.label, font, ink, false);
    }
    if chrome.enabled && response.hovered() {
        ui.ctx().set_cursor_icon(if chrome.drag { egui::CursorIcon::Grab } else { egui::CursorIcon::PointingHand });
    }
    response.on_hover_text(chrome.label)
}

/// The same full-header affordance collapses and restores with one click.
/// Reuse the existing chevron master, with the kit's vertical mirror helper.
#[allow(clippy::too_many_arguments)]
pub(crate) fn panel_host_disclosure(
    ui: &mut egui::Ui,
    id: egui::Id,
    bounds: egui::Rect,
    label: &str,
    enabled: bool,
    collapsed: bool,
    reduced: bool,
) -> egui::Response {
    let response = panel_host_control(
        ui,
        id,
        bounds,
        HostChrome {
            label,
            icon: None,
            icon_only: true,
            attention: false,
            selected: false,
            enabled,
            drag: false,
            title: false,
            grip_only: false,
            rtl: false,
        },
    );
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_expanded(!collapsed));
    let t = Tokens::get();
    let ink = if enabled { t.form_ink } else { t.form_dim };
    let extent = (bounds.width().min(bounds.height()) - t.form_icon_inset * 2.0).max(0.0);
    let icon = egui::Rect::from_center_size(bounds.center(), egui::vec2(extent, extent));
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(bounds));
    // Only the two existing glyphs blend. The response, expanded state and
    // allocated body already reflect the committed collapse state this pass.
    let duration = solidcraft_theme::panel_motion_duration(ui.ctx(), reduced);
    let expanded = ui.ctx().animate_bool_with_time_and_easing(id.with("disclosure_ink"), !collapsed, duration, solidcraft_theme::panel_motion_ease);
    if expanded < 1.0 {
        crate::icons::paint_ink(&painter, icon, "chevron", ink.linear_multiply(1.0 - expanded));
    }
    if expanded > 0.0 {
        crate::icons::paint_ink_flipped(&painter, icon, "chevron", ink.linear_multiply(expanded));
    }
    response
}

/// The owner supplies the controlled extent and axis; clipping or scrolling
/// the sash must not change its accessible value or orientation.
pub(crate) fn panel_host_sash(ui: &mut egui::Ui, id: egui::Id, bounds: egui::Rect, name: &str, horizontal: bool, extent: f32) -> egui::Response {
    let t = Tokens::get();
    let response = ui.interact(bounds, id, egui::Sense::click_and_drag());
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                egui::EventFilter {
                    horizontal_arrows: bounds.height() >= bounds.width(),
                    vertical_arrows: bounds.width() >= bounds.height(),
                    ..Default::default()
                },
            )
        });
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Unknown, ui.is_enabled(), name));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::accesskit::Role::Splitter);
        node.set_orientation(if horizontal { egui::accesskit::Orientation::Horizontal } else { egui::accesskit::Orientation::Vertical });
        node.set_numeric_value(f64::from(extent));
    });
    if response.hovered() || response.has_focus() || response.dragged() {
        ui.painter().rect_filled(bounds.intersect(ui.clip_rect()), t.radius_sm, t.form_primary);
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(if horizontal { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
    }
    response.on_hover_text(name)
}

/// A scrollbar owns only its narrow track. The surrounding canvas has no
/// response or scroll area, so uncovered gaps remain available to navigation.
#[allow(clippy::too_many_arguments)]
pub(crate) fn panel_host_scrollbar(
    ui: &mut egui::Ui,
    id: egui::Id,
    bounds: egui::Rect,
    horizontal: bool,
    visible: f32,
    total: f32,
    offset: f32,
    name: &str,
) -> (egui::Response, Option<f32>) {
    let t = Tokens::get();
    let response = ui.interact(bounds, id, egui::Sense::click_and_drag());
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(id, egui::EventFilter { horizontal_arrows: horizontal, vertical_arrows: !horizontal, ..Default::default() })
        });
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::SpinButton, ui.is_enabled(), name));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        node.set_role(egui::accesskit::Role::ScrollBar);
        node.set_orientation(if horizontal { egui::accesskit::Orientation::Horizontal } else { egui::accesskit::Orientation::Vertical });
        node.set_numeric_value(f64::from(offset));
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(f64::from((total - visible).max(0.0)));
    });
    let length = if horizontal { bounds.width() } else { bounds.height() };
    let maximum = (total - visible).max(0.0);
    if length <= 0.0 || maximum <= 0.0 {
        return (response, None);
    }
    let thumb_length = (length * visible / total).max(t.control_height.min(length)).min(length);
    let travel = length - thumb_length;
    let start = if horizontal { bounds.left() } else { bounds.top() };
    let thumb_start = start + travel * offset.clamp(0.0, maximum) / maximum;
    let thumb = if horizontal {
        egui::Rect::from_min_size(egui::pos2(thumb_start, bounds.top()), egui::vec2(thumb_length, bounds.height()))
    } else {
        egui::Rect::from_min_size(egui::pos2(bounds.left(), thumb_start), egui::vec2(bounds.width(), thumb_length))
    };
    let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(bounds));
    painter.rect_filled(bounds, t.radius_sm, t.panel_header);
    painter.rect_filled(thumb, t.radius_sm, if response.dragged() || response.hovered() { t.form_primary } else { t.form_dim });
    let mut next = None;
    if travel > 0.0
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let coordinate = if horizontal { pointer.x } else { pointer.y };
        let grab_id = id.with("scroll_grab");
        if response.drag_started() {
            ui.ctx().data_mut(|data| data.insert_temp(grab_id, (coordinate - thumb_start).clamp(0.0, thumb_length)));
        }
        if response.dragged() || response.clicked() {
            let grab = ui.ctx().data(|data| data.get_temp::<f32>(grab_id)).unwrap_or(thumb_length / 2.0);
            next = Some(((coordinate - start - grab) / travel * maximum).clamp(0.0, maximum));
        }
        if response.drag_stopped() {
            ui.ctx().data_mut(|data| data.remove::<f32>(grab_id));
        }
    }
    if response.has_focus() {
        painter.rect_stroke(bounds.shrink(t.space_2), t.radius_sm, t.control_border_stroke(t.form_primary), egui::StrokeKind::Inside);
        let delta = ui.input_mut(|input| {
            let back = if horizontal { egui::Key::ArrowLeft } else { egui::Key::ArrowUp };
            let forward = if horizontal { egui::Key::ArrowRight } else { egui::Key::ArrowDown };
            u8::from(input.consume_key(egui::Modifiers::NONE, forward)) as f32 - u8::from(input.consume_key(egui::Modifiers::NONE, back)) as f32
        });
        if delta != 0.0 {
            next = Some((offset + delta * t.control_height).clamp(0.0, maximum));
        }
    }
    (response.on_hover_text(name), next)
}

pub(crate) fn panel_host_preview(ui: &egui::Ui, bounds: egui::Rect, valid: bool, reason: Option<&str>, opacity: f32) {
    let t = Tokens::get();
    let ink = if valid { t.form_primary } else { t.form_error };
    let painter =
        ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("panel_preview"))).with_clip_rect(ui.clip_rect());
    painter.rect_filled(bounds, t.radius_md, ink.gamma_multiply(0.12 * opacity));
    let mut stroke = t.control_border_stroke(ink);
    stroke.color = stroke.color.gamma_multiply(opacity);
    painter.rect_stroke(bounds, t.radius_md, stroke, egui::StrokeKind::Inside);
    if let Some(reason) = reason {
        let galley = painter.layout(
            reason.to_owned(),
            egui::FontId::proportional(t.font_small),
            t.form_ink,
            (bounds.width() - t.space_8 * 2.0).max(t.space_2),
        );
        // Keep the rejection legible over the retained content beneath it.
        // This is paint on the existing preview layer, never an input surface.
        let reason_painter = painter.with_clip_rect(bounds.intersect(ui.clip_rect()));
        let backing = egui::Rect::from_center_size(bounds.center(), galley.size()).expand2(egui::vec2(t.space_8, t.space_4)).intersect(bounds);
        reason_painter.rect_filled(backing, t.radius_sm, t.dialog_body.to_opaque());
        reason_painter.rect_stroke(backing, t.radius_sm, t.control_border_stroke(ink.gamma_multiply(0.65)), egui::StrokeKind::Inside);
        reason_painter.galley(bounds.center() - galley.size() / 2.0, galley, t.form_ink);
    }
}

/// Paint identity on the existing preview layer without allocating a widget or
/// registering an input/occlusion footprint. Titles are already in display order;
/// never automatically wrap or reorder them. Oversized text clips to the badge.
pub(crate) fn panel_host_drag_title(ui: &egui::Ui, title: &str, at: egui::Pos2, domain: egui::Rect, rtl: bool) {
    let clip = ui.clip_rect().intersect(domain);
    if !clip.is_finite() || !clip.is_positive() || !at.is_finite() || !clip.contains(at) {
        return;
    }
    let t = Tokens::get();
    let padding = egui::vec2(t.space_8.min(clip.width() / 4.0), t.space_4.min(clip.height() / 4.0));
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("panel_preview"))).with_clip_rect(clip);
    let galley = painter.layout_no_wrap(title.to_owned(), egui::FontId::proportional(t.font_label), t.form_ink);
    let size = (galley.size() + padding * 2.0).min(clip.size());
    let x = if rtl { at.x - t.space_12 - size.x } else { at.x + t.space_12 };
    let min = egui::pos2(
        x.clamp(clip.left(), (clip.right() - size.x).max(clip.left())),
        (at.y + t.space_12).clamp(clip.top(), (clip.bottom() - size.y).max(clip.top())),
    );
    let bounds = egui::Rect::from_min_size(min, size);
    painter.rect_filled(bounds, t.radius_sm, t.dialog_body.to_opaque());
    painter.rect_stroke(bounds, t.radius_sm, t.control_border_stroke(t.dialog_border), egui::StrokeKind::Inside);
    let text_at = egui::pos2(if rtl { bounds.right() - padding.x - galley.size().x } else { min.x + padding.x }, min.y + padding.y);
    painter.with_clip_rect(bounds.intersect(clip)).galley(text_at, galley, t.form_ink);
}

pub(crate) fn panel_host_neighbour_preview(ui: &egui::Ui, bounds: egui::Rect, removed: bool, opacity: f32) {
    let t = Tokens::get();
    let painter =
        ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("panel_preview"))).with_clip_rect(ui.clip_rect());
    let stroke = egui::Stroke::new(1.0, t.form_primary.gamma_multiply(0.7 * opacity));
    painter.rect_stroke(bounds, t.radius_sm, stroke, egui::StrokeKind::Inside);
    if removed {
        painter.line_segment([bounds.left_top(), bounds.right_bottom()], stroke);
    }
}

pub(crate) fn panel_host_tab_insertion(ui: &egui::Ui, bounds: egui::Rect, opacity: f32) {
    let painter =
        ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("panel_preview"))).with_clip_rect(ui.clip_rect());
    painter.rect_filled(bounds, 0, Tokens::get().form_primary.gamma_multiply(opacity));
}

/// Menus own only their measured bounded rectangle; there is no viewport shield.
pub(crate) fn panel_host_popup<R>(ui: &mut egui::Ui, id: egui::Id, bounds: egui::Rect, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let container = Container::new(ui.ctx(), Surface::Popup);
    let frame = Container::menu_frame();
    let clip = ui.clip_rect().intersect(bounds);
    container
        .area(id)
        .order(egui::Order::Foreground)
        .fixed_pos(bounds.min)
        .movable(false)
        .fade_in(false)
        .show(ui.ctx(), |ui| {
            ui.set_clip_rect(clip);
            ui.set_min_size(bounds.size());
            Container::paint_frame(ui.painter(), frame, bounds - frame.total_margin());
            let inner = bounds.shrink(Tokens::get().space_6);
            let mut content = ui.new_child(egui::UiBuilder::new().id_salt("content").max_rect(inner).accessibility_role(egui::accesskit::Role::Menu));
            content.set_clip_rect(inner.intersect(clip));
            body(&mut content)
        })
        .inner
}
