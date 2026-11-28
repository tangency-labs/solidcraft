//! The navigation bar as a component. The app measures the row, describes each slot and each
//! menu as plain data ([`Bar`], [`SlotView`], [`Menu`]) and runs the [`NavAction`] the bar
//! returns; the bar keeps which menu is open and reads nothing of the app. Every slot, menu row
//! and context item publishes its rect to the widget registry while it is drawn.
use crate::{NavigationEdge, NavigationIdle, NavigationKind, NavigationSize};
use egui::{Rect, pos2};
use solidcraft_theme::Tokens;

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
    /// A second registry name its handle is published under, wherever it is drawn.
    pub alias: Option<&'a str>,
    /// A small dot at the icon's corner: something the slot holds is not at its default.
    pub dot: bool,
    /// Shared menu entries the slot offers above Remove. Picking one
    /// is `NavAction::Item { menu: <slot id>, item: <row id> }`.
    pub context: Vec<crate::Item>,
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
    pub entries: Vec<crate::Item>,
    /// A pick closes the menu (otherwise it stays open for the next).
    pub close_on_pick: bool,
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
    Percent {
        menu: String,
        tenths: u32,
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
    crate::navigation_surface(ui, rect, size, editing.is_some());
    let mut responses = Vec::new();
    for slot in slots {
        let response = ui
            .push_id(slot.index, |ui| {
                ui.scope(|ui| {
                    // A strong slot that is on stays at full strength while the rest of the bar fades.
                    if editing.is_none() && slot.selected && crate::navigation_bar::strong(slot.id) {
                        ui.set_opacity(1.0);
                    }
                    if slot.hidden {
                        ui.set_opacity(crate::NAV_HIDDEN_OPACITY);
                    } else if !slot.enabled {
                        ui.set_opacity(ui.visuals().disabled_alpha);
                    }
                    if slot.kind == NavigationKind::Separator {
                        crate::navigation_separator(ui, slot.rect);
                        ui.interact(
                            slot.rect,
                            ui.scope_id().with("separator"),
                            if editing.is_some() { egui::Sense::click_and_drag() } else { egui::Sense::click() },
                        )
                    } else {
                        let button = crate::NavigationButton {
                            id: slot.id,
                            kind: slot.kind,
                            icon: slot.icon,
                            label: &slot.text,
                            selected: slot.selected,
                            labels: slot.show_text,
                            menu: slot.kind == NavigationKind::Menu,
                        };
                        if editing.is_some() {
                            crate::navigation_button_editable(ui, slot.rect, button, true)
                        } else {
                            crate::navigation_button(ui, slot.rect, button)
                        }
                    }
                })
                .inner
            })
            .inner;
        let chosen = editing.map_or(slot.selected, |selected| selected == Some(slot.index));
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), chosen, &slot.name));
        if editing.is_some() {
            crate::navigation_edit_outline(ui, slot.rect, slot.hidden, chosen);
        }
        if slot.dot && editing.is_none() {
            let t = Tokens::get();
            let icon = crate::NavigationMetrics::for_size(size).icon;
            let at = pos2(slot.rect.left() + crate::NAV_MENU_LEFT + icon - 1.0, slot.rect.center().y - icon / 2.0 + 1.0);
            ui.painter().circle_filled(at, 3.8, t.dialog_body);
            ui.painter().circle_filled(at, 2.7, t.form_primary);
        }
        crate::navigation_tooltip(&response, rect, edge, &slot.tip, slot.key.as_deref());
        responses.push(response);
    }
    responses
}

/// Draw the live bar, its More button, its context menu and the open menu (`menu` gives a menu
/// slot's rows by its id); what the user chose, for the app to run.
pub fn show(ui: &mut egui::Ui, bar: &Bar<'_>, mut menu: impl FnMut(&str) -> Menu) -> Option<NavAction> {
    let menu_id = ui.scope_id().with("navigation_bar_menu");
    let mut open = ui.data(|d| d.get_temp::<String>(menu_id)).unwrap_or_default();
    let has_trigger = if open == "more" {
        bar.more.is_positive() && !bar.overflow.is_empty()
    } else {
        bar.slots.iter().any(|slot| slot.id == open && slot.rect.is_positive())
            || (bar.more.is_positive() && bar.overflow.iter().any(|slot| slot.id == open))
    };
    if !open.is_empty() && (!bar.rect.is_positive() || !has_trigger) {
        crate::Menu::dismiss(ui.ctx(), menu_id.with(("popup", &open)));
        open.clear();
        ui.data_mut(|data| data.insert_temp(menu_id, open.clone()));
    }
    if !bar.rect.is_positive() {
        return None;
    }
    solidcraft_ui_registry::publish_rect("nav:outer", bar.rect);
    let previous_open = open.clone();
    let mut action = None;
    let active = bar.active || ui.rect_contains_pointer(bar.rect) || !open.is_empty();
    let opacity = crate::navigation_opacity(bar.idle, active);
    let background = ui.interact(bar.rect, ui.scope_id().with("navigation_bar_context"), egui::Sense::click());
    let opacity = if bar.idle == NavigationIdle::Hide {
        ui.ctx().animate_value_with_time(menu_id.with("opacity"), opacity, if active { crate::NAV_FADE_SECONDS } else { 0.0 })
    } else {
        opacity
    };
    let mut anchor = bar.more;
    ui.scope(|ui| {
        ui.set_opacity(opacity);
        let shown: Vec<_> = bar.slots.iter().map(|s| SlotView { selected: s.selected || open == s.id, ..s.clone() }).collect();
        for (slot, response) in shown.iter().zip(paint_slots(ui, bar.rect, bar.size, bar.edge, &shown, None)) {
            solidcraft_ui_registry::publish_target(&format!("nav:slot:{}", slot.id), response.rect);
            if let Some(alias) = slot.alias {
                solidcraft_ui_registry::publish_handle(alias, response.rect.center());
            }
            context_menu(&response, (slot.index != usize::MAX).then_some(slot.index), Some((slot.id, &slot.context)), &bar.labels, &mut action);
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
            let response = crate::navigation_button(
                ui,
                bar.more,
                crate::NavigationButton {
                    id: "more",
                    kind: NavigationKind::Menu,
                    icon: "dots",
                    label: &bar.labels.more,
                    selected: open == "more" || bar.overflow.iter().any(|slot| slot.id == open),
                    labels: false,
                    menu: false,
                },
            );
            crate::navigation_tooltip(&response, bar.rect, bar.edge, &bar.labels.more, None);
            solidcraft_ui_registry::publish_target("nav:more", response.rect);
            context_menu(&response, None, None, &bar.labels, &mut action);
            if response.clicked() {
                open = if open == "more" { String::new() } else { "more".to_owned() };
            }
        }
    });
    // The bar's background spans its child triggers. Route a secondary release to
    // the slot under it before offering the same event to the background menu.
    let child_context = ui.input(|input| {
        input.pointer.button_released(egui::PointerButton::Secondary)
            && input.pointer.interact_pos().is_some_and(|at| bar.more.contains(at) || bar.slots.iter().any(|slot| slot.rect.contains(at)))
    });
    if child_context {
        crate::Menu::dismiss(ui.ctx(), background.id.with("shared_menu"));
    } else {
        context_menu(&background, None, None, &bar.labels, &mut action);
    }
    if !open.is_empty() && anchor.is_positive() {
        let entries = if open == "more" {
            bar.overflow
                .iter()
                .map(|slot| {
                    if slot.kind == NavigationKind::Separator {
                        return crate::Item::sep();
                    }
                    let mut item = crate::Item::action(&format!("__slot:{}", slot.id), &slot.name, slot.icon);
                    item.selected = slot.selected;
                    item.tip = Some(slot.tip.clone());
                    item.shortcut = slot.key.clone().unwrap_or_default();
                    item.aliases.push(format!("nav:slot:{}", slot.id));
                    if let Some(alias) = slot.alias {
                        item.aliases.push(alias.to_string());
                    }
                    if slot.kind == NavigationKind::Menu {
                        let contents = menu(slot.id);
                        item.children = menu_items(slot.id, &contents.entries, !contents.close_on_pick);
                        item.enabled = !item.children.is_empty();
                    }
                    item.context = context_items((slot.index != usize::MAX).then_some(slot.index), Some((slot.id, &slot.context)), &bar.labels);
                    item
                })
                .collect::<Vec<_>>()
        } else {
            let contents = menu(&open);
            menu_items(&open, &contents.entries, !contents.close_on_pick)
        };
        if entries.is_empty() {
            open.clear();
        } else {
            let out = crate::Menu::popup(ui.ctx(), menu_id.with(("popup", &open)), crate::MenuAnchor::Rect(anchor), &entries, previous_open != open);
            if let Some(picked) = out.picked {
                action = Some(nav_action(&open, &picked));
            }
            if out.close {
                open.clear();
            }
        }
    }
    ui.data_mut(|d| d.insert_temp(menu_id, open));
    action
}

/// Domain entries for a bar or slot context menu. The shared component draws every row.
fn context_items(index: Option<usize>, slot: Option<(&str, &[crate::Item])>, labels: &BarLabels) -> Vec<crate::Item> {
    let mut entries = Vec::new();
    if let Some((menu, rows)) = slot.filter(|(_, rows)| !rows.is_empty()) {
        entries.extend(rows.iter().cloned().map(|mut row| {
            row.params = Some(serde_json::json!({"nav_menu":menu}));
            row.aliases.push(format!("nav:context:{menu}:{}", row.id));
            row
        }));
        entries.push(crate::Item::sep());
    }
    let choices: Vec<(&str, &str)> = match index {
        None => vec![("customise", &labels.customise), ("hide", &labels.hide), ("reset", &labels.reset)],
        Some(_) => vec![("remove", &labels.remove)],
    };
    entries.extend(choices.into_iter().map(|(id, label)| {
        let mut item = crate::Item::action(&format!("__nav_context:{id}"), label, "");
        item.aliases.push(format!("nav:context:{id}"));
        item.params = Some(serde_json::json!({"index":index}));
        item
    }));
    entries
}

fn context_menu(
    response: &egui::Response,
    index: Option<usize>,
    slot: Option<(&str, &[crate::Item])>,
    labels: &BarLabels,
    action: &mut Option<NavAction>,
) {
    if !crate::Menu::context_active(response) {
        return;
    }
    if let Some(picked) = crate::Menu::context_items(response, &context_items(index, slot, labels)) {
        *action = Some(nav_action(slot.map_or("", |(menu, _)| menu), &picked));
    }
}

fn nav_action(menu: &str, picked: &crate::Item) -> NavAction {
    let params = picked.params.as_ref();
    if let Some(context) = picked.id.strip_prefix("__nav_context:") {
        return NavAction::Context(match context {
            "customise" => ContextItem::Customise,
            "hide" => ContextItem::Hide,
            "reset" => ContextItem::Reset,
            _ => ContextItem::Remove(params.and_then(|p| p.get("index")).and_then(serde_json::Value::as_u64).unwrap_or_default() as usize),
        });
    }
    if let Some(slot) = picked.id.strip_prefix("__slot:") {
        return NavAction::Slot(slot.to_string());
    }
    let menu = params.and_then(|p| p.get("nav_menu")).and_then(serde_json::Value::as_str).unwrap_or(menu).to_string();
    if picked.number.is_some() {
        let percent = params.and_then(|p| p.get("value")).and_then(serde_json::Value::as_f64).unwrap_or_default();
        return NavAction::Percent { menu, tenths: (percent * 10.0).round().clamp(0.0, u32::MAX as f64) as u32 };
    }
    NavAction::Item { menu, item: picked.id.clone() }
}

/// Attach routing metadata to the same Item definition used by every menu.
fn menu_items(menu: &str, entries: &[crate::Item], keep_open: bool) -> Vec<crate::Item> {
    entries
        .iter()
        .cloned()
        .map(|mut item| {
            item.keep_open |= keep_open;
            item.children = menu_items(menu, &item.children, item.keep_open);
            if !item.is_sep() && !item.is_heading() {
                let mut params = item.params.take().filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
                if let Some(params) = params.as_object_mut() {
                    params.insert("nav_menu".into(), serde_json::Value::String(menu.into()));
                }
                item.params = Some(params);
            }
            item
        })
        .collect()
}

#[cfg(test)]
#[path = "nav_bar_tests.rs"]
mod tests;
