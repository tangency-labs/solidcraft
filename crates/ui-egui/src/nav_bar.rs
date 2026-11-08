//! The navigation bar as a component. The app measures the row, describes each slot and each
//! menu as plain data ([`Bar`], [`SlotView`], [`Menu`]) and runs the [`NavAction`] the bar
//! returns; the bar keeps which menu is open and reads nothing of the app. Every slot, menu row
//! and context item publishes its rect to the widget registry while it is drawn.
use crate::ui_kit::{self, BackingTooltipExt as _, Container, NavigationEdge, NavigationIdle, NavigationKind, NavigationSize, Surface};
use egui::{Align2, Rect, pos2, vec2};

/// One slot as the bar draws it.
#[derive(Clone, Debug)]
pub struct SlotView<'a> {
    /// Where the slot is in the stored list (what Remove removes).
    pub index: usize,
    pub id: &'a str,
    pub kind: NavigationKind,
    pub icon: &'a str,
    /// The text on the button (or beside it, for a menu showing its value).
    pub text: String,
    /// The slot's name for assistive technology.
    pub name: String,
    /// Hover text, and the shortcut shown with it.
    pub tip: String,
    pub key: Option<String>,
    /// Its tool or toggle is on.
    pub selected: bool,
    /// Inert for now (drawn faded, still clickable).
    pub enabled: bool,
    /// Editing only: the slot is not shown on this canvas.
    pub hidden: bool,
    /// The button shows its text.
    pub show_text: bool,
    /// Where it is drawn (unused for a slot in the More menu).
    pub rect: Rect,
}

/// Labels of the bar's own controls, in the app's language.
#[derive(Clone, Debug, Default)]
pub struct BarLabels {
    pub more: String,
    pub customise: String,
    pub hide: String,
    pub reset: String,
    pub remove: String,
}

/// A measured bar and its slots.
#[derive(Clone, Debug)]
pub struct Bar<'a> {
    pub rect: Rect,
    pub size: NavigationSize,
    pub edge: NavigationEdge,
    pub idle: NavigationIdle,
    /// The app's reasons to show the bar at full strength (the view is being turned, the pointer
    /// is over the view compass); the pointer over the bar and an open menu count on their own.
    pub active: bool,
    pub slots: Vec<SlotView<'a>>,
    /// The slots folded into More, in order.
    pub overflow: Vec<SlotView<'a>>,
    /// The More button (`Rect::NOTHING`: nothing folded).
    pub more: Rect,
    pub labels: BarLabels,
}

/// A menu a slot opens.
#[derive(Clone, Debug, Default)]
pub struct Menu {
    pub entries: Vec<MenuEntry>,
    /// A pick closes the menu (otherwise it stays open for the next).
    pub close_on_pick: bool,
}

#[derive(Clone, Debug)]
pub enum MenuEntry {
    Heading(String),
    Separator,
    Row(MenuRow),
}

/// One row of a slot's menu.
#[derive(Clone, Debug, Default)]
pub struct MenuRow {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub selected: bool,
    pub enabled: bool,
    /// Text at the row's end (a current value).
    pub detail: Option<String>,
    pub tip: Option<String>,
    /// The registry name its rect is published under.
    pub target: String,
    /// Rows of the menu this row opens (empty: the row is picked itself).
    pub submenu: Vec<MenuRow>,
}

impl MenuRow {
    pub fn new(id: impl Into<String>, label: impl Into<String>, icon: impl Into<String>, target: impl Into<String>) -> MenuRow {
        MenuRow { id: id.into(), label: label.into(), icon: icon.into(), enabled: true, target: target.into(), ..MenuRow::default() }
    }
}

/// An item of the bar's context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextItem {
    Customise,
    Hide,
    Reset,
    /// Remove the slot at this index of the stored list.
    Remove(usize),
}

/// What the user chose on the bar this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavAction {
    /// A slot that is not a menu was clicked (on the bar or in More).
    Slot(String),
    /// Row `item` of the menu of slot `menu` (or of its submenu) was picked.
    Item {
        menu: String,
        item: String,
    },
    Context(ContextItem),
}

/// Paint the bar's surface and its slots; the slots' responses in order. The live bar and the
/// Preferences editor (`editing`: the selected slot's index, if any) share it.
pub fn paint_slots(
    ui: &mut egui::Ui,
    rect: Rect,
    size: NavigationSize,
    edge: NavigationEdge,
    slots: &[SlotView<'_>],
    editing: Option<Option<usize>>,
) -> Vec<egui::Response> {
    ui_kit::navigation_surface(ui, rect, size, editing.is_some());
    let mut responses = Vec::new();
    for slot in slots {
        let response = ui
            .push_id(slot.index, |ui| {
                ui.scope(|ui| {
                    if slot.hidden {
                        ui.set_opacity(ui_kit::NAV_HIDDEN_OPACITY);
                    } else if !slot.enabled {
                        ui.set_opacity(ui.visuals().disabled_alpha);
                    }
                    if slot.kind == NavigationKind::Separator {
                        ui_kit::navigation_separator(ui, slot.rect);
                        ui.interact(
                            slot.rect,
                            ui.scope_id().with("separator"),
                            if editing.is_some() { egui::Sense::click_and_drag() } else { egui::Sense::click() },
                        )
                    } else {
                        let button = ui_kit::NavigationButton {
                            id: slot.id,
                            kind: slot.kind,
                            icon: slot.icon,
                            label: &slot.text,
                            selected: slot.selected,
                            labels: slot.show_text,
                            menu: slot.kind == NavigationKind::Menu,
                        };
                        if editing.is_some() {
                            ui_kit::navigation_button_editable(ui, slot.rect, button, true)
                        } else {
                            ui_kit::navigation_button(ui, slot.rect, button)
                        }
                    }
                })
                .inner
            })
            .inner;
        let chosen = editing.map_or(slot.selected, |selected| selected == Some(slot.index));
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), chosen, &slot.name));
        if editing.is_some() {
            ui_kit::navigation_edit_outline(ui, slot.rect, slot.hidden, chosen);
        }
        ui_kit::navigation_tooltip(&response, rect, edge, &slot.tip, slot.key.as_deref());
        responses.push(response);
    }
    responses
}

/// Draw the live bar, its More button, its context menu and the open menu (`menu` gives a menu
/// slot's rows by its id); what the user chose, for the app to run.
pub fn show(ui: &mut egui::Ui, bar: &Bar<'_>, menu: impl FnOnce(&str) -> Menu) -> Option<NavAction> {
    if !bar.rect.is_positive() {
        return None;
    }
    solidcraft_ui_registry::publish_rect("nav:outer", bar.rect);
    let menu_id = ui.scope_id().with("navigation_bar_menu");
    let mut open = ui.data(|d| d.get_temp::<String>(menu_id)).unwrap_or_default();
    let mut action = None;
    let active = bar.active || ui.rect_contains_pointer(bar.rect) || !open.is_empty();
    let opacity = ui_kit::navigation_opacity(bar.idle, active);
    let background = ui.interact(bar.rect, ui.scope_id().with("navigation_bar_context"), egui::Sense::click());
    let opacity = if bar.idle == NavigationIdle::Hide {
        ui.ctx().animate_value_with_time(menu_id.with("opacity"), opacity, if active { ui_kit::NAV_FADE_SECONDS } else { 0.0 })
    } else {
        opacity
    };
    let mut anchor = bar.more;
    ui.scope(|ui| {
        ui.set_opacity(opacity);
        let shown: Vec<_> = bar.slots.iter().map(|s| SlotView { selected: s.selected || open == s.id, ..s.clone() }).collect();
        for (slot, response) in shown.iter().zip(paint_slots(ui, bar.rect, bar.size, bar.edge, &shown, None)) {
            solidcraft_ui_registry::publish_target(&format!("nav:slot:{}", slot.id), response.rect);
            context_menu(&response, Some(slot.index), &bar.labels, &mut action);
            if response.clicked() && slot.kind != NavigationKind::Separator {
                if slot.kind == NavigationKind::Menu {
                    open = if open == slot.id { String::new() } else { slot.id.to_owned() };
                } else {
                    action = Some(NavAction::Slot(slot.id.to_owned()));
                }
            }
            if open == slot.id {
                anchor = response.rect;
            }
        }
        if bar.more.is_positive() {
            let response = ui_kit::navigation_button(
                ui,
                bar.more,
                ui_kit::NavigationButton {
                    id: "more",
                    kind: NavigationKind::Menu,
                    icon: "dots",
                    label: &bar.labels.more,
                    selected: open == "more" || bar.overflow.iter().any(|slot| slot.id == open),
                    labels: false,
                    menu: false,
                },
            );
            ui_kit::navigation_tooltip(&response, bar.rect, bar.edge, &bar.labels.more, None);
            solidcraft_ui_registry::publish_target("nav:more", response.rect);
            context_menu(&response, None, &bar.labels, &mut action);
            if response.clicked() {
                open = if open == "more" { String::new() } else { "more".to_owned() };
            }
        }
    });
    context_menu(&background, None, &bar.labels, &mut action);
    if !open.is_empty() && anchor.is_positive() {
        let child_popup_open = egui::Popup::is_any_open(ui.ctx());
        let ctx = ui.ctx().clone();
        // Menus open into the view: up from a bar at the bottom, down from one at the top.
        let (pivot, at) = match bar.edge {
            NavigationEdge::Bottom => (Align2::RIGHT_BOTTOM, anchor.right_top() - vec2(0.0, ui_kit::NAV_POPUP_GAP)),
            NavigationEdge::Top => (Align2::RIGHT_TOP, anchor.right_bottom() + vec2(0.0, ui_kit::NAV_POPUP_GAP)),
        };
        let contents = if open == "more" { None } else { Some(menu(&open)) };
        let popup = Container::new(&ctx, Surface::Popup).area(menu_id.with("popup")).order(egui::Order::Foreground).pivot(pivot).fixed_pos(at).show(
            &ctx,
            |ui| {
                ui_kit::navigation_menu(ui, if open == "more" { ui_kit::NAV_MORE_WIDTH } else { ui_kit::NAV_POPUP_WIDTH }, |ui| match &contents {
                    None => overflow_menu(ui, bar, &mut open, &mut action),
                    Some(m) if m.entries.is_empty() => open.clear(),
                    Some(m) => {
                        if let Some(item) = menu_rows(ui, &m.entries) {
                            action = Some(NavAction::Item { menu: open.clone(), item });
                            if m.close_on_pick {
                                open.clear();
                            }
                        }
                    }
                });
            },
        );
        let outside = !child_popup_open
            && ui.input(|i| {
                i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !popup.response.rect.contains(p) && !bar.rect.contains(p))
            });
        if outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            open.clear();
        }
    }
    ui.data_mut(|d| d.insert_temp(menu_id, open));
    action
}

/// The context menu of the bar (`index`: `None`) or of a slot: Customise, Hide and Reset on the
/// bar, Remove on a slot.
fn context_menu(response: &egui::Response, index: Option<usize>, labels: &BarLabels, action: &mut Option<NavAction>) {
    ui_kit::navigation_context(response, |ui| {
        let items: Vec<(&str, &str, ContextItem)> = match index {
            None => vec![
                ("customise", &labels.customise, ContextItem::Customise),
                ("hide", &labels.hide, ContextItem::Hide),
                ("reset", &labels.reset, ContextItem::Reset),
            ],
            Some(index) => vec![("remove", &labels.remove, ContextItem::Remove(index))],
        };
        for (id, label, item) in items {
            let row = ui_kit::NavigationMenuRow::new(id, label, "").show(ui);
            solidcraft_ui_registry::publish_target(&format!("nav:context:{id}"), row.rect);
            if row.clicked() {
                *action = Some(NavAction::Context(item));
                ui.close();
            }
        }
    });
}

/// More: the folded slots as rows. A menu slot opens its menu in place of More.
fn overflow_menu(ui: &mut egui::Ui, bar: &Bar<'_>, open: &mut String, action: &mut Option<NavAction>) {
    for slot in &bar.overflow {
        if slot.kind == NavigationKind::Separator {
            let start = ui.next_widget_position();
            ui_kit::navigation_menu_separator(ui);
            let bounds = Rect::from_min_max(start, pos2(start.x + ui.available_width(), ui.next_widget_position().y));
            solidcraft_ui_registry::publish_target(&format!("nav:slot:{}", slot.id), bounds);
            continue;
        }
        let mut item = ui_kit::NavigationMenuRow::new(slot.id, &slot.name, slot.icon);
        if matches!(slot.kind, NavigationKind::Tool | NavigationKind::Toggle) {
            item.selected = slot.selected;
        }
        let response = item.show(ui);
        context_menu(&response, Some(slot.index), &bar.labels, action);
        solidcraft_ui_registry::publish_target(&format!("nav:slot:{}", slot.id), response.rect);
        ui_kit::navigation_tooltip(&response, response.rect, NavigationEdge::Bottom, &slot.tip, slot.key.as_deref());
        if response.clicked() {
            if slot.kind == NavigationKind::Menu {
                *open = slot.id.to_owned();
            } else {
                *action = Some(NavAction::Slot(slot.id.to_owned()));
                open.clear();
            }
        }
    }
}

/// A menu's rows; the id of the row picked, if any.
fn menu_rows(ui: &mut egui::Ui, entries: &[MenuEntry]) -> Option<String> {
    let mut picked = None;
    for entry in entries {
        match entry {
            MenuEntry::Heading(title) => ui_kit::navigation_menu_heading(ui, title),
            MenuEntry::Separator => ui_kit::navigation_menu_separator(ui),
            MenuEntry::Row(row) => {
                let response = menu_row(ui, row);
                if row.submenu.is_empty() {
                    if response.clicked() {
                        picked = Some(row.id.clone());
                    }
                    continue;
                }
                Container::new(ui.ctx(), Surface::Popup).menu(&response).frame(ui_kit::navigation_popup_frame()).show(|ui| {
                    ui.set_width(ui_kit::NAV_POPUP_WIDTH - 2.0 * (f32::from(ui_kit::NAV_POPUP_PADDING) + ui_kit::NAV_BORDER));
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for sub in &row.submenu {
                        if menu_row(ui, sub).clicked() {
                            picked = Some(sub.id.clone());
                            ui.close();
                        }
                    }
                });
            }
        }
    }
    picked
}

fn menu_row(ui: &mut egui::Ui, row: &MenuRow) -> egui::Response {
    let mut item = ui_kit::NavigationMenuRow::new(&row.id, &row.label, &row.icon);
    item.selected = row.selected;
    item.enabled = row.enabled;
    item.detail = row.detail.as_deref();
    item.submenu = !row.submenu.is_empty();
    let response = item.show(ui);
    let response = match &row.tip {
        Some(tip) => response.backing_on_hover_text(tip),
        None => response,
    };
    if !row.target.is_empty() {
        solidcraft_ui_registry::publish_target(&row.target, response.rect);
    }
    response
}
