//! Persistent viewport status content. The caller owns items and dispatches returned actions.
use super::{Button, Container, Surface};
use crate::BackingTooltipExt as _;
use crate::type_face::RichTextExt as _;
use egui::{Id, Rect, Response, RichText, Ui, vec2};
use solidcraft_theme::Tokens;

/// Shape and outline communicate the kind independently of colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusKind {
    Mode,
    Result,
    Attention,
}

impl StatusKind {
    pub fn icon(self) -> &'static str {
        match self {
            Self::Mode => "eye",
            Self::Result => "status_pin",
            Self::Attention => "warning",
        }
    }
}

/// Presentation data, already translated and formatted in the design's units.
/// A later engine adapter supplies this without storing session state in the widget.
pub struct StatusItem {
    pub id: String,
    pub kind: StatusKind,
    pub name: String,
    pub setting: String,
    pub has_key: bool,
    pub editable: bool,
    /// The key that turns this effect off, for the close button's hint; `None` keeps the hint's
    /// Escape.
    pub clear_key: Option<String>,
}

/// The close button's hint: its own key when the effect names one, else Escape's.
fn clear_hint(item: &StatusItem) -> String {
    match &item.clear_key {
        Some(key) => format!("{} · {key}", solidcraft_ui_text::format("ui.viewport_status_clear", &[("name", &item.name)])),
        None => solidcraft_ui_text::format("ui.viewport_status_clear_hint", &[("name", &item.name)]),
    }
}

#[derive(Default)]
pub struct StatusPresentation {
    pub open_key: Option<String>,
    pub focus_field: Option<String>,
    pub selected_record: Option<String>,
    pub live_details: bool,
    return_focus: Option<Id>,
    return_pending: bool,
    pub card_owner: Option<String>,
    pub opened_frame: Option<u64>,
    pub session_uid: Option<u64>,
    focus_ids: Vec<Id>,
    pub summary_popup: Option<Id>,
    pub summary_members: Vec<String>,
    pub summary_suspended: bool,
}

impl StatusPresentation {
    /// A card and its focus/popup identities never follow a different document's equal slot ID.
    pub fn reconcile_session(&mut self, ctx: &egui::Context, uid: u64) {
        if self.session_uid.is_some_and(|previous| previous != uid) {
            super::dismiss_details_popups(ctx);
            if let Some(popup) = self.summary_popup {
                super::Menu::dismiss(ctx, popup);
            }
            if self.owns_focus(ctx) || super::details_field_owns_focus(ctx) {
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
            }
            *self = Self::default();
        }
        self.session_uid = Some(uid);
    }
    /// Only popup/focus identities are retained; every displayed value comes from this frame.
    pub fn reconcile_summary(&mut self, ctx: &egui::Context, members: Vec<String>) {
        if self.summary_members != members {
            super::dismiss_details_popups(ctx);
            if self.open_key.is_some() && (super::details_field_owns_focus(ctx) || super::details_control_owns_focus(ctx)) {
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
            }
            self.return_pending = false;
            self.card_owner = None;
            if let Some(id) = self.summary_popup.take() {
                super::Menu::dismiss(ctx, id);
            }
            if self.owns_focus(ctx) {
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
            }
            self.focus_ids.clear();
            self.open_key = None;
            self.focus_field = None;
            self.summary_members = members;
        }
    }

    /// A higher owner dismisses only this popup, without returning focus to its badge.
    pub fn suspend_summary(&mut self, ctx: &egui::Context, suspended: bool) {
        if suspended {
            if self.live_details {
                super::dismiss_details_popups(ctx);
            }
            if self.open_key.is_some() && (super::details_field_owns_focus(ctx) || super::details_control_owns_focus(ctx)) {
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
            }
            self.focus_field = None;
            self.return_pending = false;
        }
        self.summary_suspended = suspended;
        if suspended && !self.summary_members.is_empty() {
            if let Some(id) = self.summary_popup.take() {
                super::Menu::dismiss(ctx, id);
            }
            if self.owns_focus(ctx) {
                ctx.memory_mut(|memory| {
                    if let Some(id) = memory.focused() {
                        memory.surrender_focus(id);
                    }
                });
            }
            self.focus_ids.clear();
        }
    }

    pub fn owns_focus(&self, ctx: &egui::Context) -> bool {
        ctx.memory(|memory| memory.focused().is_some_and(|id| self.focus_ids.contains(&id)))
            || (self.open_key.is_some() && super::details_control_owns_focus(ctx))
    }

    pub fn close_details(&mut self) {
        self.open_key = None;
        self.focus_field = None;
        self.return_pending = true;
        self.card_owner = None;
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StatusAction {
    Clear(String),
    ClearAll,
    Edit(String),
}

fn target(item: &StatusItem, part: &str) -> String {
    format!("viewport_status:{}:{part}", item.id)
}

/// Diameter of a pill's round chevron and ✕ buttons.
const ACTION_DIAMETER: f32 = 22.0;

fn icon_action(ui: &mut Ui, item: &StatusItem, part: &str, icon: &str, label: &str, hint: &str, flipped: bool) -> Response {
    let t = Tokens::get();
    let height = ui.spacing().interact_size.y.max(t.control_height);
    let (slot, _) = ui.allocate_exact_size(vec2(t.space_24, height), egui::Sense::hover());
    // A round button with no frame: egui's own button would draw a square outline on hover
    // and focus, so the circle, its hover fill and its focus ring are painted here.
    let rect = Rect::from_center_size(slot.center(), vec2(ACTION_DIAMETER, ACTION_DIAMETER));
    let response = ui.interact(rect, Id::unique(target(item, part)), egui::Sense::click());
    if response.clicked() {
        response.request_focus();
    }
    let radius = ACTION_DIAMETER / 2.0;
    if response.hovered() || response.is_pointer_button_down_on() {
        ui.painter().circle_filled(rect.center(), radius, t.hover);
    }
    if response.has_focus() {
        ui.painter().circle_stroke(rect.center(), radius + t.space_2, ui.visuals().selection.stroke);
    }
    if response.enabled() && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let glyph = Rect::from_center_size(response.rect.center(), vec2(t.space_24, t.space_24) - vec2(t.space_4, t.space_4) * 2.0);
    if flipped {
        crate::icons::paint_ink_flipped(ui.painter(), glyph, icon, t.text_dim);
    } else {
        crate::icons::paint_icon(ui.painter(), glyph, icon, 1.0);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    solidcraft_ui_registry::publish_target(&target(item, part), response.rect);
    solidcraft_ui_registry::publish_target(&target(item, &format!("{part}_glyph")), glyph);
    response.backing_on_hover_text(hint)
}

/// One pill, with native keyboard controls and no engine or camera input.
pub fn viewport_status_item(ui: &mut Ui, item: &StatusItem, state: &mut StatusPresentation) -> (Rect, Option<StatusAction>) {
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
            let name_height = ui.painter().layout_no_wrap(item.name.clone(), crate::type_face::semibold(t.font_label), t.text).size().y;
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
                solidcraft_ui_registry::publish_target(&target(item, "icon"), disc_rect);
                let name = ui.label(RichText::new(&item.name).size(t.font_label).semibold().color(t.text));
                solidcraft_ui_registry::publish_target(&target(item, "name"), name.rect);
                if !item.setting.is_empty() {
                    let text = RichText::new(&item.setting).monospace().size(t.font_small).color(t.text_dim);
                    let response = if item.editable { super::button(ui, text, Button::Ghost) } else { ui.label(text) };
                    solidcraft_ui_registry::publish_target(&target(item, "setting"), response.rect);
                    if item.editable {
                        state.focus_ids.push(response.id);
                    }
                    if item.editable && response.clicked() {
                        action = Some(StatusAction::Edit(item.id.clone()));
                    }
                }
                if item.has_key {
                    let open = state.open_key.as_deref() == Some(&item.id);
                    let label = solidcraft_ui_text::format("ui.viewport_status_key", &[("name", &item.name)]);
                    let response = icon_action(ui, item, "key", "chevron", &label, &label, open);
                    state.focus_ids.push(response.id);
                    if open || response.clicked() || matches!(&action, Some(StatusAction::Edit(id)) if id == &item.id) {
                        state.return_focus = Some(response.id);
                    }
                    if state.return_pending
                        && state.return_focus == Some(response.id)
                        && !(egui::Popup::is_any_open(ui.ctx()) || super::Menu::any_open(ui.ctx()))
                    {
                        response.request_focus();
                        state.return_pending = false;
                    }
                    if response.clicked() {
                        state.open_key = if open { None } else { Some(item.id.clone()) };
                    }
                }
                let label = solidcraft_ui_text::format("ui.viewport_status_clear", &[("name", &item.name)]);
                let hint = clear_hint(item);
                let response = icon_action(ui, item, "close", "close", &label, &hint, false);
                state.focus_ids.push(response.id);
                if response.clicked() {
                    action = Some(StatusAction::Clear(item.id.clone()));
                }
            });
        })
        .response;
    let focus = ui.interact(response.rect, Id::unique(target(item, "item")), egui::Sense::focusable_noninteractive());
    state.focus_ids.push(focus.id);
    focus.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, ui.is_enabled(), format!("{} {}", item.name, item.setting)));
    if focus.has_focus() {
        ui.painter().rect_stroke(response.rect.expand(t.space_3), frame.corner_radius, ui.visuals().selection.stroke, egui::StrokeKind::Outside);
    }
    solidcraft_ui_registry::publish_target(&target(item, "item"), response.rect);
    (response.rect, action)
}

/// One oldest-first row. Only actual pills claim input; empty viewport space stays free.
pub fn viewport_status_row(
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
        .area(Id::unique("viewport_status_row"))
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
    solidcraft_ui_registry::publish_rect("viewport_status:row", row.response.rect);
    solidcraft_ui_registry::publish_count("viewport_status:count", items.len() as f64);
    if state.return_pending && state.return_focus.is_none_or(|id| !state.focus_ids.contains(&id)) {
        state.return_pending = false;
        state.return_focus = None;
    }
    if let Some((anchor, id)) = key_anchor {
        show_card(ctx, free, anchor, row.response.rect, id, state, &mut key_contents);
    } else {
        state.card_owner = None;
        state.focus_field = None;
    }
    actions
}

/// Count folding is independent of viewport geometry. Newest is supplied by Session chronology.
pub fn viewport_status_summary(
    ctx: &egui::Context,
    free: Rect,
    items: &[StatusItem],
    newest: &str,
    state: &mut StatusPresentation,
    mut key_contents: impl FnMut(&mut Ui, &str),
) -> Vec<StatusAction> {
    let Some(latest) = items.iter().find(|item| item.id == newest) else { return Vec::new() };
    state.focus_ids.clear();
    if !items.iter().any(|item| item.has_key && state.open_key.as_deref() == Some(&item.id)) {
        state.open_key = None;
    }
    let t = Tokens::get();
    let mut actions = Vec::new();
    let container = Container::new(ctx, Surface::ViewportStatus);
    let was_open = state.summary_popup.is_some_and(|id| super::Menu::is_open(ctx, id));
    let row = container
        .area(Id::unique("viewport_status_summary"))
        .movable(false)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(egui::pos2(free.center().x, free.top() + t.space_8))
        .show(ctx, |ui| {
            let (outline, disc) = match latest.kind {
                StatusKind::Mode => (t.accent, t.accent_soft),
                StatusKind::Result => (t.border, t.text_dim.gamma_multiply(0.1)),
                StatusKind::Attention => (t.warning, t.warning.gamma_multiply(0.12)),
            };
            let frame = container.frame(ctx, t.overlay, crate::frame::Side::Left).stroke(t.primary_border(outline)).inner_margin(t.space_4);
            container.show_frame(ui, frame, free, |ui| {
                // The background badge cannot open a foreground popup above a higher owner.
                if state.summary_suspended {
                    ui.disable();
                }
                let response = {
                    let extra =
                        solidcraft_ui_text::format("ui.viewport_status_summary_more", &[("count", &items.len().saturating_sub(1).to_string())]);
                    let name = ui.painter().layout_no_wrap(latest.name.clone(), crate::type_face::semibold(t.font_label), t.text);
                    let count = ui.painter().layout_no_wrap(extra, egui::FontId::monospace(t.font_small), t.text_dim);
                    let attention = items.iter().any(|item| item.kind == StatusKind::Attention);
                    let width = name.size().x + count.size().x + t.space_24 * 2.0 + t.space_8 * 4.0 + if attention { t.space_12 } else { 0.0 };
                    let (rect, _) = ui.allocate_exact_size(vec2(width, t.control_height.max(ui.spacing().interact_size.y)), egui::Sense::hover());
                    let response = super::button_at(ui, "viewport_status_summary_button", rect, "", Button::Ghost);
                    if response.clicked() {
                        response.request_focus();
                    }
                    let popup_id = response.id.with("shared_menu");
                    if response.has_focus() {
                        ui.painter().rect_stroke(rect, t.control_radius, ui.visuals().selection.stroke, egui::StrokeKind::Outside);
                    }
                    let mut x = rect.left() + t.space_8;
                    let icon =
                        Rect::from_center_size(egui::pos2(x + t.space_12, rect.center().y), vec2(t.space_24 - t.space_4, t.space_24 - t.space_4));
                    ui.painter().circle_filled(icon.center(), icon.width() / 2.0, disc);
                    crate::icons::paint_icon(ui.painter(), icon.shrink(t.space_3), latest.kind.icon(), 1.0);
                    x += t.space_24 + t.space_8;
                    ui.painter().galley(egui::pos2(x, rect.center().y - name.size().y / 2.0), name.clone(), t.text);
                    x += name.size().x + t.space_8;
                    ui.painter().galley(egui::pos2(x, rect.center().y - count.size().y / 2.0), count.clone(), t.text_dim);
                    x += count.size().x + t.space_8;
                    if attention {
                        let dot = egui::pos2(x + t.space_3, rect.center().y);
                        ui.painter().circle_filled(dot, t.space_3, t.warning);
                        solidcraft_ui_registry::publish_target(
                            "viewport_status:summary:attention",
                            Rect::from_center_size(dot, vec2(t.space_6, t.space_6)),
                        );
                        x += t.space_12;
                    }
                    let chevron = Rect::from_center_size(egui::pos2(x + t.space_12, rect.center().y), vec2(t.space_16, t.space_16));
                    if super::Menu::is_open(ctx, popup_id) {
                        crate::icons::paint_ink_flipped(ui.painter(), chevron, "chevron", t.text_dim);
                    } else {
                        crate::icons::paint_icon(ui.painter(), chevron, "chevron", 1.0);
                    }
                    let label = solidcraft_ui_text::format(
                        "ui.viewport_status_summary_label",
                        &[("name", &latest.name), ("count", &items.len().saturating_sub(1).to_string())],
                    );
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label.as_str()));
                    solidcraft_ui_registry::publish_target("viewport_status:summary", rect);
                    solidcraft_ui_registry::publish_target(&target(latest, "summary"), rect);
                    response
                };
                let button_id = Some(response.id);
                let summary_popup = Some(response.id.with("shared_menu"));
                let menu = if response.clicked() || super::Menu::response_is_open(&response) {
                    let mut entries = vec![super::Item::heading(solidcraft_ui_text::tr("ui.viewport_status_summary_heading"))];
                    for item in items {
                        let mut children = Vec::new();
                        if item.editable {
                            let mut edit = super::Item::action(&target(item, "setting"), &item.setting, "");
                            edit.params = Some(serde_json::json!({"status":item.id,"kind":"edit"}));
                            edit.aliases.push(target(item, "setting"));
                            children.push(edit);
                        }
                        if item.has_key {
                            let label = solidcraft_ui_text::format("ui.viewport_status_key", &[("name", &item.name)]);
                            let mut key = super::Item::action(&target(item, "key"), &label, "chevron");
                            key.params = Some(serde_json::json!({"status":item.id,"kind":"key"}));
                            key.aliases.push(target(item, "key"));
                            children.push(key);
                        }
                        let label = solidcraft_ui_text::format("ui.viewport_status_clear", &[("name", &item.name)]);
                        let mut clear = super::Item::action(&target(item, "close"), &label, "close");
                        clear.tip = Some(clear_hint(item));
                        clear.params = Some(serde_json::json!({"status":item.id,"kind":"clear"}));
                        clear.aliases.push(target(item, "close"));
                        let mut row = super::Item::submenu(&target(item, "item"), &item.name, item.kind.icon(), children);
                        row.secondary = Some(Box::new(clear));
                        row.shortcut = item.setting.clone();
                        row.aliases.extend([target(item, "item"), target(item, "name"), target(item, "icon")]);
                        entries.push(row);
                    }
                    entries.push(super::Item::sep());
                    let mut clear =
                        super::Item::action("viewport_status_clear_all", solidcraft_ui_text::tr("ui.viewport_status_summary_clear"), "").key("Esc");
                    clear.params = Some(serde_json::json!({"kind":"clear_all"}));
                    entries.push(clear);
                    super::Menu::button_items_output(&response, &entries)
                } else {
                    super::MenuOutput::default()
                };
                state.focus_ids.extend(menu.rows.iter().filter(|(_, response)| response.enabled()).map(|(_, response)| response.id));
                if let Some(picked) = menu.picked {
                    let params = picked.params.unwrap_or_default();
                    let id = params.get("status").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
                    match params.get("kind").and_then(serde_json::Value::as_str) {
                        Some("edit") => actions.push(StatusAction::Edit(id)),
                        Some("key") => state.open_key = Some(id),
                        Some("clear") => actions.push(StatusAction::Clear(id)),
                        Some("clear_all") => actions.push(StatusAction::ClearAll),
                        _ => {}
                    }
                }
                state.summary_popup = summary_popup;
                state.focus_ids.extend(button_id);
                if state.open_key.is_some() {
                    state.return_focus = button_id;
                }
                if state.return_pending && state.return_focus == button_id && !(egui::Popup::is_any_open(ctx) || super::Menu::any_open(ctx)) {
                    response.request_focus();
                    state.return_pending = false;
                }
                if was_open && !summary_popup.is_some_and(|id| super::Menu::is_open(ctx, id)) && actions.is_empty() {
                    response.request_focus();
                }
                if response.clicked() {
                    state.open_key = None;
                }
            });
        });
    solidcraft_ui_registry::publish_rect("viewport_status:row", row.response.rect);
    solidcraft_ui_registry::publish_count("viewport_status:count", items.len() as f64);
    if let Some(id) = state.open_key.clone() {
        show_card(ctx, free, row.response.rect.left_bottom(), row.response.rect, &id, state, &mut key_contents);
    } else {
        state.card_owner = None;
        state.focus_field = None;
    }
    actions
}

/// The details card under a pill: an opaque surface, so the model never shows through its rows,
/// with a hairline border, a soft shadow and the card radius.
pub fn viewport_status_card_frame() -> egui::Frame {
    let t = Tokens::get();
    Container::menu_frame()
        .fill(t.dialog_body.to_opaque())
        .corner_radius(t.radius_lg)
        .inner_margin(egui::Margin::symmetric(t.space_12 as i8, t.space_10 as i8))
}

pub fn show_card(
    ctx: &egui::Context,
    free: Rect,
    anchor: egui::Pos2,
    owner: Rect,
    id: &str,
    state: &mut StatusPresentation,
    contents: &mut impl FnMut(&mut Ui, &str),
) {
    if state.live_details && state.summary_suspended {
        return;
    }
    if state.card_owner.as_deref() != Some(id) {
        state.card_owner = Some(id.to_owned());
        state.opened_frame = Some(ctx.cumulative_frame_nr());
    }
    let popup_was_open = egui::Popup::is_any_open(ctx) || super::Menu::any_open(ctx);
    let t = Tokens::get();
    let card = Container::new(ctx, Surface::Popup);
    let result =
        card.area(Id::unique("viewport_status_key")).order(egui::Order::Background).movable(false).fixed_pos(anchor + vec2(0.0, t.space_6)).show(
            ctx,
            |ui| {
                card.show_frame(ui, viewport_status_card_frame(), free, |ui| contents(ui, id));
            },
        );
    solidcraft_ui_registry::publish_target("viewport_status:card", result.response.rect);
    if state.opened_frame != Some(ctx.cumulative_frame_nr())
        && !popup_was_open
        && !(egui::Popup::is_any_open(ctx) || super::Menu::any_open(ctx))
        && ctx.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|point| !result.response.rect.contains(point) && !owner.contains(point))
        })
    {
        state.close_details();
    }
}
