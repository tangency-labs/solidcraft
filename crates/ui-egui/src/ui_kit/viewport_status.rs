//! Persistent viewport status content. The caller owns items and dispatches returned actions.
use super::{Button, Container, Surface};
use crate::{fonts::RichTextExt as _, scenario, theme::Tokens};
use egui::{Id, Rect, Response, RichText, Ui, vec2};

/// Shape and outline communicate the kind independently of colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusKind {
    Mode,
    Result,
    Attention,
}

impl StatusKind {
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Self::Mode => "eye",
            Self::Result => "status_pin",
            Self::Attention => "warning",
        }
    }
}

/// Presentation data, already translated and formatted in the design's units.
/// A later engine adapter supplies this without storing session state in the widget.
pub(crate) struct StatusItem {
    pub id: String,
    pub kind: StatusKind,
    pub name: String,
    pub setting: String,
    pub has_key: bool,
    pub editable: bool,
}

#[derive(Default)]
pub(crate) struct StatusPresentation {
    pub open_key: Option<String>,
    focus_ids: Vec<Id>,
}

impl StatusPresentation {
    pub(crate) fn owns_focus(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|memory| memory.focused().is_some_and(|id| self.focus_ids.contains(&id)))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StatusAction {
    Clear(String),
    Edit(String),
}

fn target(item: &StatusItem, part: &str) -> String {
    format!("viewport_status:{}:{part}", item.id)
}

fn icon_action(ui: &mut Ui, item: &StatusItem, part: &str, icon: &str, label: &str, hint: &str, flipped: bool) -> Response {
    let t = Tokens::get();
    let height = ui.spacing().interact_size.y.max(t.control_height);
    let (rect, _) = ui.allocate_exact_size(vec2(t.space_24, height), egui::Sense::hover());
    let response = super::button_at(ui, &target(item, part), rect, "", Button::Ghost);
    if response.clicked() {
        response.request_focus();
    }
    // Ghost keeps native button interaction but intentionally omits egui's frame.
    // Status actions still need the local hover target required by this element.
    if response.hovered() {
        ui.painter().rect_filled(response.rect, t.control_radius, t.hover);
    }
    let glyph = Rect::from_center_size(response.rect.center(), vec2(t.space_24, t.space_24) - vec2(t.space_4, t.space_4) * 2.0);
    if flipped {
        crate::icons::paint_ink_flipped(ui.painter(), glyph, icon, t.text_dim);
    } else {
        crate::icons::paint_icon(ui.painter(), glyph, icon, 1.0);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    scenario::publish_target(&target(item, part), response.rect);
    scenario::publish_target(&target(item, &format!("{part}_glyph")), glyph);
    response.on_hover_text(hint)
}

/// One pill, with native keyboard controls and no engine or camera input.
pub(crate) fn viewport_status_item(ui: &mut Ui, item: &StatusItem, state: &mut StatusPresentation) -> (Rect, Option<StatusAction>) {
    let t = Tokens::get();
    let (outline, disc) = match item.kind {
        StatusKind::Mode => (t.accent, t.accent_soft),
        StatusKind::Result => (t.border, t.text_dim.gamma_multiply(0.1)),
        StatusKind::Attention => (t.warning, t.warning.gamma_multiply(0.12)),
    };
    let container = Container::new(ui.ctx(), Surface::ViewportStatus);
    let frame = container
        .frame(ui.ctx(), t.overlay, crate::frame::Side::Left)
        .stroke(t.primary_border(outline))
        .inner_margin(egui::Margin::symmetric(t.space_8 as i8, t.space_4 as i8));
    let mut action = None;
    let response = container
        .show_frame(ui, frame, ui.max_rect(), |ui| {
            // Bound the cross-axis before laying out any part. Otherwise a later native
            // button can grow the row after its earlier, shorter contents were positioned.
            let name_height = ui.painter().layout_no_wrap(item.name.clone(), crate::fonts::semibold(t.font_label), t.text).size().y;
            let setting_height = ui.painter().layout_no_wrap(item.setting.clone(), egui::FontId::monospace(t.font_small), t.text_dim).size().y;
            let height = t.control_height.max(ui.spacing().interact_size.y).max(name_height).max(setting_height);
            let size = vec2(ui.available_width(), height);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.spacing_mut().item_spacing.x = t.space_6;
                ui.spacing_mut().interact_size.y = height;
                let disc_size = t.space_24 - t.space_4;
                let (disc_rect, _) = ui.allocate_exact_size(vec2(disc_size, disc_size), egui::Sense::hover());
                ui.painter().circle_filled(disc_rect.center(), disc_rect.width() / 2.0, disc);
                crate::icons::paint_icon(ui.painter(), disc_rect.shrink(t.space_3), item.kind.icon(), 1.0);
                scenario::publish_target(&target(item, "icon"), disc_rect);
                let name = ui.label(RichText::new(&item.name).size(t.font_label).semibold().color(t.text));
                scenario::publish_target(&target(item, "name"), name.rect);
                if !item.setting.is_empty() {
                    let text = RichText::new(&item.setting).monospace().size(t.font_small).color(t.text_dim);
                    let response = if item.editable { super::button(ui, text, Button::Ghost) } else { ui.label(text) };
                    scenario::publish_target(&target(item, "setting"), response.rect);
                    if item.editable {
                        state.focus_ids.push(response.id);
                    }
                    if item.editable && response.clicked() {
                        action = Some(StatusAction::Edit(item.id.clone()));
                    }
                }
                if item.has_key {
                    let open = state.open_key.as_deref() == Some(&item.id);
                    let label = crate::i18n::format("ui.viewport_status_key", &[("name", &item.name)]);
                    let response = icon_action(ui, item, "key", "chevron", &label, &label, open);
                    state.focus_ids.push(response.id);
                    if response.clicked() {
                        state.open_key = if open { None } else { Some(item.id.clone()) };
                    }
                }
                let label = crate::i18n::format("ui.viewport_status_clear", &[("name", &item.name)]);
                let hint = crate::i18n::format("ui.viewport_status_clear_hint", &[("name", &item.name)]);
                let response = icon_action(ui, item, "close", "close", &label, &hint, false);
                state.focus_ids.push(response.id);
                if response.clicked() {
                    action = Some(StatusAction::Clear(item.id.clone()));
                }
            });
        })
        .response;
    let focus = ui.interact(response.rect, Id::new(target(item, "item")), egui::Sense::focusable_noninteractive());
    state.focus_ids.push(focus.id);
    focus.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), format!("{} {}", item.name, item.setting)));
    if focus.has_focus() {
        ui.painter().rect_stroke(response.rect.expand(t.space_3), frame.corner_radius, ui.visuals().selection.stroke, egui::StrokeKind::Outside);
    }
    scenario::publish_target(&target(item, "item"), response.rect);
    (response.rect, action)
}

/// One oldest-first row. Only actual pills claim input; empty viewport space stays free.
pub(crate) fn viewport_status_row(
    ctx: &egui::Context,
    free: Rect,
    items: &[StatusItem],
    state: &mut StatusPresentation,
    mut key_contents: impl FnMut(&mut Ui, &str),
) -> Vec<StatusAction> {
    state.focus_ids.clear();
    if items.is_empty() {
        state.open_key = None;
        return Vec::new();
    }
    if !items.iter().any(|item| item.has_key && state.open_key.as_deref() == Some(&item.id)) {
        state.open_key = None;
    }
    let t = Tokens::get();
    let container = Container::new(ctx, Surface::ViewportStatus);
    let mut actions = Vec::new();
    let mut key_anchor = None;
    let row = container
        .area(Id::new("viewport_status_row"))
        .movable(false)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(egui::pos2(free.center().x, free.top() + t.space_8))
        .show(ctx, |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = t.space_8;
                for item in items {
                    let (rect, action) = viewport_status_item(ui, item, state);
                    if state.open_key.as_deref() == Some(&item.id) {
                        key_anchor = Some((rect.left_bottom(), item.id.as_str()));
                    }
                    if let Some(action) = action {
                        actions.push(action);
                    }
                }
            });
        });
    scenario::publish_rect("viewport_status:row", row.response.rect);
    scenario::publish_count("viewport_status:count", items.len() as f64);
    if let Some((anchor, id)) = key_anchor {
        let card = Container::new(ctx, Surface::Popup);
        // The key belongs to viewport chrome, not to the system-window/popup layer.
        let result =
            card.area(Id::new("viewport_status_key")).order(egui::Order::Background).movable(false).fixed_pos(anchor + vec2(0.0, t.space_6)).show(
                ctx,
                |ui| {
                    let frame = card.frame(ctx, t.overlay, crate::frame::Side::Left).inner_margin(t.space_12);
                    card.show_frame(ui, frame, free, |ui| key_contents(ui, id));
                },
            );
        scenario::publish_target("viewport_status:card", result.response.rect);
    }
    actions
}

/// Shared legend content; the caller supplies the existing analysis-token swatches and text.
pub(crate) fn analysis_key(ui: &mut Ui, title: &str, along: Option<&str>, rows: &[(egui::Color32, String, bool)]) {
    let t = Tokens::get();
    ui.label(RichText::new(title).semibold().size(t.font_label).color(t.text));
    if let Some(along) = along {
        ui.label(RichText::new(along).monospace().size(t.font_small).color(t.text_dim));
    }
    for (colour, label, hatched) in rows {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(t.space_12, t.space_12), egui::Sense::hover());
            ui.painter().rect_filled(rect, t.radius_sm, *colour);
            if *hatched {
                let painter = ui.painter().with_clip_rect(rect);
                for offset in [-t.space_8, -t.space_4, 0.0, t.space_4, t.space_8] {
                    let from = rect.left_bottom() + vec2(offset, 0.0);
                    painter
                        .line_segment([from, from + vec2(rect.height(), -rect.height())], egui::Stroke::new(t.primary_border(t.text).width, t.text));
                }
            }
            ui.label(RichText::new(label).size(t.font_label).color(t.text));
        });
    }
}
