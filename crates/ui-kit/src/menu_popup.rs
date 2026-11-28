//! The one declarative menu renderer: rows, recursive flyouts, scrolling and input.
use super::{Item, Menu, MenuItem};
use crate::{BackingTooltipExt as _, Container, Surface};
use egui::{Context, FontId, Id, Pos2, Rect, Response, Sense, Vec2, pos2, vec2};
use solidcraft_theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MenuAnchor {
    Point(Pos2),
    Rect(Rect),
}

#[derive(Clone, Debug, Default)]
pub struct MenuOutput {
    pub picked: Option<Item>,
    pub close: bool,
    pub hovered: Option<Item>,
    pub active_row: Option<Id>,
    pub open: bool,
    pub rect: Option<Rect>,
    pub inside: bool,
    pub depth: usize,
    /// The root is measuring invisibly; hosts must not expose its geometry yet.
    pub sizing: bool,
    pub rows: Vec<(Item, Response)>,
}

#[derive(Clone, Default)]
struct PopupState {
    pointer_gate: Option<(usize, Rect)>,
    focus_rows: Vec<(Id, usize, usize)>,
    auxiliary_rows: Vec<Id>,
    visible_rects: Vec<Rect>,
    source_layer: Option<egui::LayerId>,
    last_focus: Option<Id>,
    path: Vec<usize>,
    open_ids: Vec<String>,
    cursor: Vec<Option<usize>>,
    leaving: Option<(usize, f64)>,
    frame: u64,
    open: bool,
    keyboard: bool,
    editing: bool,
    seeded: bool,
    scroll_cursor: bool,
}
#[derive(Clone, Default)]
struct MarkingState {
    press: Option<(Pos2, f64)>,
    flick: bool,
}

#[derive(Clone)]
struct AdapterState {
    anchor: MenuAnchor,
    open: bool,
    fresh: bool,
}
#[derive(Default)]
struct Seen {
    sizing: bool,
    ids: Vec<(Id, usize)>,
    auxiliary: Vec<Id>,
    anchor: Option<Rect>,
    opens: Option<usize>,
    opens_anchor: Option<Rect>,
    other_row: bool,
}
const MAX_DEPTH: usize = 32;
const MAX_ROWS: usize = 4096;

fn state_id(ctx: &Context, id: Id) -> Id {
    id.with(("shared_menu_state", ctx.viewport_id()))
}
fn active_key(ctx: &Context) -> Id {
    Id::unique(("shared_menu_keyboard", ctx.viewport_id()))
}

fn adapter_id(ctx: &Context, id: Id) -> Id {
    id.with(("shared_menu_adapter", ctx.viewport_id()))
}

fn levels<'a>(items: &'a [Item], path: &[usize]) -> Vec<&'a [Item]> {
    let mut rows = vec![items];
    for &index in path.iter().take(MAX_DEPTH) {
        let Some(item) =
            rows.last().and_then(|list| list.get(index)).filter(|item| item.enabled && (item.lazy_children || !item.children.is_empty()))
        else {
            break;
        };
        rows.push(&item.children);
    }
    rows
}
fn height(items: &[Item]) -> f32 {
    let t = Tokens::get();
    Menu::frame().total_margin().sum().y
        + items
            .iter()
            .take(MAX_ROWS)
            .map(|item| {
                if item.is_sep() {
                    t.space_10 + 1.0
                } else if item.is_heading() {
                    t.space_24
                } else {
                    t.control_height + t.space_2
                }
            })
            .sum::<f32>()
}
fn width(ctx: &Context, items: &[Item], screen: Rect) -> f32 {
    let t = Tokens::get();
    let width = ctx.fonts_mut(|fonts| {
        items
            .iter()
            .take(MAX_ROWS)
            .filter(|item| !item.is_sep())
            .map(|item| {
                let label =
                    fonts.layout_no_wrap(solidcraft_ui_text::tr_label(&item.label).to_string(), FontId::proportional(t.font_label), t.form_ink);
                let key = fonts.layout_no_wrap(
                    if item.shortcut == "current" { "".into() } else { item.shortcut.clone() },
                    FontId::monospace(t.font_small),
                    t.form_dim,
                );
                t.control_height
                    + t.space_2
                    + label.size().x
                    + key.size().x
                    + t.space_24
                    + if item.children.is_empty() && !item.lazy_children { 0.0 } else { t.space_16 }
                    + if item.checked.is_some() && !item.icon.is_empty() { t.space_24 } else { 0.0 }
                    + if item.secondary.is_some() { t.control_height } else { 0.0 }
                    + if item.number.is_some() { t.control_height * 3.0 } else { 0.0 }
                    + t.space_16 * item.indent.min(16) as f32
            })
            .fold(190.0, f32::max)
    });
    let room = (screen.width() - Menu::frame().total_margin().sum().x * 2.0).max(1.0);
    width.min(room.min(super::LIST_W * 1.5))
}
pub fn place(at: Pos2, gap: Vec2, size: Vec2, screen: Rect) -> Pos2 {
    let axis = |value: f32, size: f32, lo: f32, hi: f32| {
        if value + size <= hi {
            value
        } else if value - size >= lo {
            value - size
        } else {
            (hi - size).max(lo)
        }
    };
    let with_gap = |value: f32, gap: f32, size: f32, lo: f32, hi: f32| {
        if value + gap + size <= hi {
            value + gap
        } else if value - gap - size >= lo {
            value - gap - size
        } else {
            axis(value, size, lo, hi)
        }
    };
    pos2(with_gap(at.x, gap.x, size.x, screen.left(), screen.right()), with_gap(at.y, gap.y, size.y, screen.top(), screen.bottom()))
}
fn draw_level(
    ui: &mut egui::Ui,
    items: &[Item],
    depth: usize,
    cursor: Option<usize>,
    open: Option<usize>,
    leaving: bool,
    output: &mut MenuOutput,
    editing: &mut bool,
    scroll_cursor: bool,
    keyboard: bool,
    deepest: bool,
) -> Seen {
    let t = Tokens::get();
    let mut seen = Seen { sizing: ui.is_sizing_pass(), ..Default::default() };
    let mut on_open = false;
    ui.spacing_mut().item_spacing.y = 0.0;
    for (index, item) in items.iter().take(MAX_ROWS).enumerate() {
        if item.is_sep() {
            Menu::separator(ui);
            continue;
        }
        if item.is_heading() {
            Menu::heading(ui, &item.label);
            continue;
        }
        let mut row = MenuItem::from_item(item, (keyboard && cursor == Some(index)) || (open == Some(index) && !leaving));
        row.publish_handle = false;
        row.suppress_pointer_hot = true;
        let secondary_width = if item.secondary.is_some() { t.control_height } else { 0.0 };
        let number_width = if item.number.is_some() { t.control_height * 3.0 } else { 0.0 };
        let extra = secondary_width + number_width;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height + t.space_2), Sense::hover());
        let primary = Rect::from_min_max(rect.min, pos2((rect.right() - extra).max(rect.left()), rect.bottom()));
        let response = ui.interact(
            primary,
            item.widget_id.unwrap_or_else(|| ui.make_persistent_id(("entry", &item.id, index))),
            if item.draggable && item.enabled {
                Sense::click_and_drag()
            } else if item.draggable {
                Sense::drag()
            } else if item.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        row.highlighted |= !keyboard && response.hovered();
        row.paint(ui, rect, &response, extra);
        if item.enabled && ui.is_enabled() && ((keyboard && deepest && cursor == Some(index)) || (!keyboard && response.hovered())) {
            output.active_row = Some(response.id);
        }
        seen.ids.push((response.id, index));
        let first_id = !output.rows.iter().any(|(previous, _)| previous.id == item.id);
        let first_label = !output.rows.iter().any(|(previous, _)| previous.label == item.label);
        let first_pair = !output.rows.iter().any(|(previous, _)| previous.id == item.id && previous.label == item.label);
        output.rows.push((item.clone(), response.clone()));
        if ui.is_enabled() && !item.context.is_empty() {
            let context = Menu::context_items_output(&response, &item.context);
            output.inside |= context.inside;
            output.rows.extend(context.rows);
            if context.hovered.is_some() {
                output.hovered = context.hovered;
            }
            if context.picked.is_some() {
                output.picked = context.picked;
            }
        }
        // An id never depends on translated text; label aliases preserve existing scripts.
        for handle in first_id
            .then(|| format!("menu:{}", item.id))
            .into_iter()
            .chain(first_label.then(|| format!("menu:{}", item.label)))
            .chain(first_pair.then(|| format!("menu:{}:{}", item.id, item.label)))
            .chain(item.aliases.iter().filter(|_| item.number.is_none()).cloned())
        {
            solidcraft_ui_registry::publish_target(&handle, primary);
        }
        if scroll_cursor && cursor == Some(index) && ui.is_enabled() {
            Menu::focus_row(&response);
        }
        if response.has_focus() {
            Menu::lock_row(&response);
        }
        if let Some(tip) = &item.tip {
            response.clone().backing_on_hover_text(tip);
        }
        if response.hovered() {
            output.hovered = Some(item.clone());
        }
        if let Some(secondary) = &item.secondary {
            let rect = Rect::from_min_max(pos2(rect.right() - secondary_width, rect.top()), rect.max);
            let response = ui.interact(rect, response.id.with("secondary"), if secondary.enabled { Sense::click() } else { Sense::hover() });
            seen.auxiliary.push(response.id);
            if response.has_focus() {
                Menu::lock_row(&response);
            }
            let mut control = MenuItem::from_item(secondary, false);
            control.show_label = false;
            // The icon identifies this compact button; its popup does not add a row chevron.
            control.submenu = false;
            control.paint(ui, rect, &response, 0.0);
            for handle in std::iter::once(format!("menu:{}", secondary.id)).chain(secondary.aliases.iter().cloned()) {
                solidcraft_ui_registry::publish_target(&handle, rect);
            }
            if let Some(tip) = &secondary.tip {
                response.clone().backing_on_hover_text(tip);
            }
            if response.hovered() {
                if !keyboard && secondary.enabled && ui.is_enabled() {
                    output.active_row = Some(response.id);
                }
                output.hovered = Some((**secondary).clone());
            }
            output.rows.push(((**secondary).clone(), response.clone()));
            if !secondary.children.is_empty() || secondary.lazy_children {
                let keyboard_dismiss = ui.input(|input| input.key_pressed(egui::Key::Escape) || input.key_pressed(egui::Key::ArrowLeft));
                let child = Menu::button_items_output(&response, &secondary.children);
                if child.close && child.picked.is_none() && keyboard_dismiss {
                    Menu::focus_row(&response);
                }
                output.inside |= child.inside;
                if child.active_row.is_some() {
                    output.active_row = child.active_row;
                }
                output.rows.extend(child.rows);
                if child.hovered.is_some() {
                    output.hovered = child.hovered;
                }
                if child.picked.is_some() {
                    output.picked = child.picked;
                }
            } else if response.clicked() && secondary.enabled {
                output.picked = Some((**secondary).clone());
            }
        }
        if let Some(number) = &item.number {
            let rect = Rect::from_min_max(pos2(rect.right() - extra, rect.top()), pos2(rect.right() - secondary_width, rect.bottom()))
                .shrink2(vec2(t.space_4, 0.0));
            let mut value = if number.value.is_finite() { number.value } else { 0.0 };
            let min = if number.min.is_finite() { number.min } else { 0.0 };
            let max = if number.max.is_finite() { number.max.max(min) } else { min.max(1.0) };
            let speed = if number.speed.is_finite() && number.speed > 0.0 { number.speed } else { 1.0 };
            let response = ui
                .add_enabled_ui(item.enabled, |ui| {
                    ui.put(rect, egui::DragValue::new(&mut value).range(min..=max).speed(speed).suffix(&number.suffix))
                })
                .inner;
            seen.auxiliary.push(response.id);
            output.rows.push((item.clone(), response.clone()));
            solidcraft_ui_registry::publish_target(&format!("menu:{}:value", item.id), response.rect);
            for handle in &item.aliases {
                solidcraft_ui_registry::publish_target(handle, response.rect);
            }
            *editing |= response.has_focus() || response.is_pointer_button_down_on();
            if response.changed() {
                let mut picked = item.clone();
                let mut params = item.params.clone().filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
                params["value"] = serde_json::json!(value);
                picked.params = Some(params);
                picked.keep_open = true;
                output.picked = Some(picked);
            }
        }
        if item.enabled && response.clicked() && ((item.children.is_empty() && !item.lazy_children) || item.submenu_action) && item.number.is_none() {
            output.picked = Some(item.clone());
        } else if item.enabled && (!item.children.is_empty() || item.lazy_children) && (Menu::flyout_hover(&response) || response.clicked()) {
            seen.opens = Some(index);
            seen.opens_anchor = Some(rect);
        }
        if item.children.is_empty() && !item.lazy_children {
            seen.other_row |= response.hovered();
        }
        if open == Some(index) {
            seen.anchor = Some(rect);
            on_open |= response.hovered();
        }
    }
    seen.other_row |= !on_open && ui.rect_contains_pointer(ui.min_rect());
    let _ = depth;
    seen
}

impl Menu {
    /// Land on a menu action without a second spatial move from egui.
    pub fn focus_row(response: &Response) {
        response.request_focus();
        response.ctx.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        Self::lock_row(response);
    }
    fn lock_row(response: &Response) {
        response.ctx.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter { horizontal_arrows: true, vertical_arrows: true, escape: true, ..Default::default() },
            )
        });
    }
    /// Whether the pointer is inside a live, visibly painted shared menu in this viewport.
    pub fn contains_pointer(ctx: &Context) -> bool {
        ctx.pointer_hover_pos().is_some_and(|pointer| Self::contains_point(ctx, pointer))
    }
    /// Whether a point belongs to a live, visibly painted shared menu in this viewport.
    pub fn contains_point(ctx: &Context, pointer: Pos2) -> bool {
        let viewport = ctx.viewport_id();
        let frame = ctx.cumulative_frame_nr();
        let registry = Id::unique(("shared_menus", viewport));
        ctx.data(|data| {
            data.get_temp::<Vec<(Id, u64, bool)>>(registry).is_some_and(|menus| {
                menus.iter().any(|(id, seen, open)| {
                    *open
                        && seen.saturating_add(1) >= frame
                        && data
                            .get_temp::<PopupState>(id.with(("shared_menu_state", viewport)))
                            .is_some_and(|state| state.open && state.visible_rects.iter().any(|rect| rect.contains(pointer)))
                })
            })
        })
    }
    /// Whether the pointer is in a live shared menu descended from this form layer.
    pub fn contains_pointer_in(ctx: &Context, ancestor: egui::LayerId) -> bool {
        let Some(pointer) = ctx.pointer_hover_pos() else { return false };
        let viewport = ctx.viewport_id();
        let frame = ctx.cumulative_frame_nr();
        let registry = Id::unique(("shared_menus", viewport));
        ctx.data(|data| {
            data.get_temp::<Vec<(Id, u64, bool)>>(registry).is_some_and(|menus| {
                menus.iter().any(|(id, seen, open)| {
                    if !*open || seen.saturating_add(1) < frame {
                        return false;
                    }
                    let Some(state) = data.get_temp::<PopupState>(id.with(("shared_menu_state", viewport))) else { return false };
                    if !state.open || !state.visible_rects.iter().any(|rect| rect.contains(pointer)) {
                        return false;
                    }
                    let mut source = state.source_layer;
                    for _ in 0..MAX_DEPTH {
                        let Some(layer) = source else { return false };
                        if layer == ancestor {
                            return true;
                        }
                        let owner_key = layer.id.with(("shared_menu_owner", viewport));
                        let Some((owner, owner_layer, seen)) = data.get_temp::<(Id, egui::LayerId, u64)>(owner_key) else { return false };
                        if owner_layer != layer || seen.saturating_add(1) < frame {
                            return false;
                        }
                        source = data
                            .get_temp::<PopupState>(owner.with(("shared_menu_state", viewport)))
                            .filter(|parent| parent.open && parent.frame.saturating_add(1) >= frame)
                            .and_then(|parent| parent.source_layer);
                    }
                    false
                })
            })
        })
    }
    /// Canonical active action, independent of stationary pointer hover during key navigation.
    pub fn row_is_hot(response: &Response) -> bool {
        let key = Id::unique(("shared_menu_hot_rows", response.ctx.viewport_id()));
        let frame = response.ctx.cumulative_pass_nr();
        response
            .ctx
            .data(|data| data.get_temp::<Vec<(Id, u64)>>(key))
            .is_some_and(|rows| rows.iter().any(|(id, seen)| *id == response.id && *seen == frame))
    }
    pub fn any_open(ctx: &Context) -> bool {
        let frame = ctx.cumulative_frame_nr();
        let key = Id::unique(("shared_menus", ctx.viewport_id()));
        ctx.data(|data| data.get_temp::<Vec<(Id, u64, bool)>>(key))
            .is_some_and(|entries| entries.iter().any(|(_, seen, open)| *open && seen.saturating_add(1) >= frame))
    }
    /// Optional press-and-hold opening gesture for a marking presentation.
    pub fn marking_trigger(ctx: &Context, id: Id, enabled: bool, can_start: bool) -> Option<Pos2> {
        let key = state_id(ctx, id).with("marking");
        let mut state = ctx.data(|data| data.get_temp::<MarkingState>(key)).unwrap_or_default();
        let mut requested = None;
        if !enabled {
            state = MarkingState::default();
        } else {
            let (pressed, down, point, time) = ctx.input(|input| {
                (input.pointer.button_pressed(egui::PointerButton::Secondary), input.pointer.secondary_down(), input.pointer.latest_pos(), input.time)
            });
            if pressed && can_start {
                state.press = point.map(|point| (point, time));
            }
            if let Some((start, since)) = state.press {
                if !down || point.is_some_and(|point| point.distance(start) > 5.0) {
                    state.press = None;
                } else if time - since > 0.35 {
                    state.press = None;
                    state.flick = true;
                    requested = Some(start);
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_millis(30));
                }
            }
        }
        ctx.data_mut(|data| data.insert_temp(key, state));
        requested
    }
    pub fn marking_center(at: Pos2, screen: Rect) -> Pos2 {
        let radius = crate::RADIAL_RADIUS + Tokens::get().space_4;
        let axis = |value: f32, low: f32, high: f32| {
            if high - low > radius * 2.0 { value.clamp(low + radius, high - radius) } else { (low + high) * 0.5 }
        };
        pos2(axis(at.x, screen.left(), screen.right()), axis(at.y, screen.top(), screen.bottom()))
    }
    pub fn marking_sector(offset: Vec2) -> Option<usize> {
        if !offset.is_finite() || offset.length() < 22.0 {
            return None;
        }
        let angle = offset.x.atan2(-offset.y).to_degrees();
        Some((((angle + 360.0 + 22.5) / 45.0).floor() as i64).rem_euclid(8) as usize)
    }
    pub fn marking_list_place(center: Pos2, list_height: f32, screen: Rect) -> Pos2 {
        let radius = crate::RADIAL_RADIUS;
        let gap = Tokens::get().space_12;
        let below = center.y + radius + gap;
        if below + list_height <= screen.bottom() - 4.0 {
            return pos2(
                (center.x - super::LIST_W / 2.0).clamp(screen.left() + 4.0, (screen.right() - super::LIST_W - 4.0).max(screen.left() + 4.0)),
                below,
            );
        }
        let top = (center.y - list_height / 2.0).clamp(screen.top() + 4.0, (screen.bottom() - list_height - 4.0).max(screen.top() + 4.0));
        let right = center.x + radius + gap;
        if right + super::LIST_W <= screen.right() - 4.0 {
            pos2(right, top)
        } else {
            pos2((center.x - radius - gap - super::LIST_W).max(screen.left() + 4.0), top)
        }
    }
    /// A marking ring is an optional presentation over the same items, output and dismissal state.
    pub fn marking_popup(ctx: &Context, id: Id, at: Pos2, items: &[Item], slots: &[Item], just_opened: bool) -> MenuOutput {
        let center = Self::marking_center(at, ctx.content_rect());
        let radius = crate::RADIAL_RADIUS + Tokens::get().space_4;
        let ring = Rect::from_center_size(center, vec2(radius * 2.0, radius * 2.0));
        let pointer = ctx.pointer_hover_pos();
        let hot = pointer.and_then(|point| Self::marking_sector(point - center));
        let key = state_id(ctx, id).with("marking");
        let mut state = ctx.data(|data| data.get_temp::<MarkingState>(key)).unwrap_or_default();
        if ctx.input(|input| input.pointer.button_pressed(egui::PointerButton::Secondary)) && pointer.is_some_and(|point| ring.contains(point)) {
            state.flick = true;
        }
        let mut picked = None;
        Container::new(ctx, Surface::Popup)
            .area(id.with("marking-ring"))
            .kind(egui::UiKind::Menu)
            .fixed_pos(ring.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let _ = ui.allocate_exact_size(ring.size(), Sense::hover());
                picked = crate::radial_menu(ui, center, slots, hot);
            });
        if state.flick && ctx.input(|input| input.pointer.button_released(egui::PointerButton::Secondary)) {
            state.flick = false;
            if pointer.is_some_and(|point| point.distance(center) > 36.0) {
                picked = hot.and_then(|index| slots.get(index)).filter(|item| item.enabled).cloned().or(picked);
            }
        }
        ctx.data_mut(|data| data.insert_temp(key, state));
        let position = Self::marking_list_place(center, height(items), ctx.content_rect());
        let mut output =
            Self::render(ctx, id, MenuAnchor::Point(position), items, just_opened, None, true, egui::Order::Foreground, Some(ring), None);
        if output.close && output.picked.is_none() {
            return output;
        }
        if let Some(item) = picked {
            output.close = !item.keep_open;
            output.open = !output.close;
            output.picked = Some(item);
            if output.close {
                Self::dismiss(ctx, id);
            }
        }
        output
    }
    pub fn is_open(ctx: &Context, id: Id) -> bool {
        let key = state_id(ctx, id);
        let frame = ctx.cumulative_frame_nr();
        ctx.data(|data| data.get_temp::<PopupState>(key)).is_some_and(|state| state.open && state.frame.saturating_add(1) >= frame)
    }
    /// Visible outer bounds of a popup level in this viewport (the root is level zero).
    pub fn level_rect(ctx: &Context, id: Id, depth: usize) -> Option<Rect> {
        if depth > MAX_DEPTH || !Self::is_open(ctx, id) || depth > Self::open_path(ctx, id).len() {
            return None;
        }
        let area = state_id(ctx, id).with(("level", depth));
        ctx.memory(|memory| memory.area_rect(area))
    }
    pub fn response_is_open(response: &Response) -> bool {
        let id = response.id.with("shared_menu");
        let key = adapter_id(&response.ctx, id);
        let state = response.ctx.data(|data| data.get_temp::<AdapterState>(key));
        state.is_some_and(|state| state.open && (state.fresh || Self::is_open(&response.ctx, id)))
    }
    pub fn dismiss(ctx: &Context, id: Id) {
        if Self::is_open(ctx, id) {
            ctx.request_repaint();
        }
        let key = state_id(ctx, id);
        let active = active_key(ctx);
        let adapter = adapter_id(ctx, id);
        let registry = Id::unique(("shared_menus", ctx.viewport_id()));
        ctx.data_mut(|data| {
            data.remove::<PopupState>(key);
            data.remove::<MarkingState>(key.with("marking"));
            if data.get_temp::<(Id, u64)>(active).is_some_and(|(menu, _)| menu == id) {
                data.remove::<(Id, u64)>(active);
            }
            data.remove::<AdapterState>(adapter);
            let entries = data.get_temp_mut_or_default::<Vec<(Id, u64, bool)>>(registry);
            entries.retain(|(menu, _, _)| *menu != id);
        });
    }
    pub fn height(items: &[Item]) -> f32 {
        height(items)
    }
    pub fn open_path(ctx: &Context, id: Id) -> Vec<usize> {
        let key = state_id(ctx, id);
        ctx.data(|data| data.get_temp::<PopupState>(key)).map_or_else(Vec::new, |state| state.path)
    }
    pub fn branch_open(ctx: &Context, id: Id, item_id: &str) -> bool {
        let key = state_id(ctx, id);
        ctx.data(|data| data.get_temp::<PopupState>(key)).is_some_and(|state| state.open_ids.iter().any(|id| id == item_id))
    }
    pub fn cursor(ctx: &Context, id: Id, depth: usize) -> Option<usize> {
        let key = state_id(ctx, id);
        ctx.data(|data| data.get_temp::<PopupState>(key)).and_then(|state| state.cursor.get(depth).copied().flatten())
    }
    pub fn set_cursor(ctx: &Context, id: Id, depth: usize, index: Option<usize>) {
        if depth >= MAX_DEPTH {
            return;
        }
        let key = state_id(ctx, id);
        ctx.data_mut(|data| {
            let state = data.get_temp_mut_or_default::<PopupState>(key);
            state.cursor.resize(depth + 1, None);
            state.cursor[depth] = index;
            state.seeded = true;
            state.scroll_cursor = true;
        });
    }
    /// Reveal an item by stable id, opening its ancestors and scrolling its row into view.
    pub fn reveal(ctx: &Context, id: Id, items: &[Item], item_id: &str) {
        fn find(items: &[Item], id: &str, path: &mut Vec<usize>) -> bool {
            if path.len() >= MAX_DEPTH {
                return false;
            }
            for (index, item) in items.iter().take(MAX_ROWS).enumerate() {
                path.push(index);
                if item.id == id || find(&item.children, id, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let mut path = Vec::new();
        if !find(items, item_id, &mut path) {
            return;
        }
        let key = state_id(ctx, id);
        ctx.data_mut(|data| {
            let state = data.get_temp_mut_or_default::<PopupState>(key);
            state.cursor = path.iter().map(|index| Some(*index)).collect();
            path.pop();
            state.path = path;
            state.seeded = true;
            state.scroll_cursor = true;
        });
    }
    pub fn open_button(response: &Response) {
        let key = adapter_id(&response.ctx, response.id.with("shared_menu"));
        response.ctx.data_mut(|data| data.insert_temp(key, AdapterState { anchor: MenuAnchor::Rect(response.rect), open: true, fresh: true }));
        response.ctx.request_repaint();
    }
    pub fn context_active(response: &Response) -> bool {
        response.enabled() && (Self::response_is_open(response) || Self::context_requested(response))
    }
    fn context_requested(response: &Response) -> bool {
        if !response.enabled() {
            return false;
        }
        let ctx = &response.ctx;
        let frame = ctx.cumulative_frame_nr();
        let press_key = response.id.with(("shared_context_press", ctx.viewport_id()));
        let (pressed, down, released, origin) = ctx.input(|input| {
            (
                input.pointer.button_pressed(egui::PointerButton::Secondary),
                input.pointer.secondary_down(),
                input.pointer.button_released(egui::PointerButton::Secondary),
                input.pointer.press_origin().or_else(|| {
                    input.events.iter().find_map(|event| match event {
                        egui::Event::PointerButton { pos, button: egui::PointerButton::Secondary, pressed: true, .. } => Some(*pos),
                        _ => None,
                    })
                }),
            )
        });
        let previous = ctx.data(|data| data.get_temp::<(u64, bool)>(press_key));
        let started_here = if pressed {
            response.contains_pointer() && origin.is_some_and(|origin| response.rect.contains(origin))
        } else {
            previous.is_some_and(|(seen, started)| seen.saturating_add(1) >= frame && started)
        };
        if pressed || down || released {
            ctx.data_mut(|data| data.insert_temp(press_key, (frame, started_here)));
        }
        let inside = response.contains_pointer()
            && ctx.pointer_interact_pos().is_some_and(|point| response.rect.contains(point) && ctx.layer_id_at(point) == Some(response.layer_id));
        let focused = response.has_focus();
        response.secondary_clicked()
            || (inside && released && started_here)
            || ctx.input(|input| focused && input.modifiers.shift && input.key_pressed(egui::Key::F10))
    }
    pub fn context_items(response: &Response, items: &[Item]) -> Option<Item> {
        Self::context_items_output(response, items).picked
    }
    pub fn button_items(response: &Response, items: &[Item]) -> Option<Item> {
        Self::button_items_output(response, items).picked
    }
    pub fn context_items_output(response: &Response, items: &[Item]) -> MenuOutput {
        Self::adapter(response, items, true)
    }
    pub fn button_items_output(response: &Response, items: &[Item]) -> MenuOutput {
        Self::adapter(response, items, false)
    }
    fn adapter(response: &Response, items: &[Item], context: bool) -> MenuOutput {
        let ctx = &response.ctx;
        let id = response.id.with("shared_menu");
        if items.is_empty() || !response.enabled() {
            Self::dismiss(ctx, id);
            return MenuOutput::default();
        }
        let adapter = adapter_id(ctx, id);
        let mut state = ctx.data(|data| data.get_temp::<AdapterState>(adapter));
        // Owners may stop rendering while a menu is open (a hidden tab, toolbar
        // group or parent popup). Its adapter must not resurrect the expired popup.
        if state.as_ref().is_some_and(|state| state.open && !state.fresh && !Self::is_open(ctx, id)) {
            Self::dismiss(ctx, id);
            state = None;
        }
        if context && Self::context_requested(response) {
            state = Some(AdapterState {
                anchor: MenuAnchor::Point(ctx.pointer_interact_pos().unwrap_or(response.rect.center())),
                open: true,
                fresh: true,
            });
        } else if !context && response.clicked() && response.enabled() {
            state =
                Some(AdapterState { anchor: MenuAnchor::Rect(response.rect), open: !state.as_ref().is_some_and(|state| state.open), fresh: true });
        }
        let Some(mut state) = state else {
            return MenuOutput::default();
        };
        if !state.open {
            ctx.data_mut(|data| data.insert_temp(adapter, state));
            return MenuOutput::default();
        }
        let source = state_id(ctx, id).with("source_layer");
        let frame = ctx.cumulative_frame_nr();
        ctx.data_mut(|data| data.insert_temp(source, (response.layer_id, frame)));
        let output = Self::popup(ctx, id, state.anchor, items, state.fresh);
        state.fresh = false;
        state.open = !output.close;
        ctx.data_mut(|data| data.insert_temp(adapter, state));
        output
    }
    pub fn popup(ctx: &Context, id: Id, anchor: MenuAnchor, items: &[Item], just_opened: bool) -> MenuOutput {
        Self::render(ctx, id, anchor, items, just_opened, None, true, egui::Order::Foreground, None, None)
    }
    /// Show a popup constrained to the host's finite viewport capacity.
    pub fn popup_with_bounds(ctx: &Context, id: Id, anchor: MenuAnchor, items: &[Item], just_opened: bool, bounds: Rect) -> MenuOutput {
        Self::render(ctx, id, anchor, items, just_opened, None, true, egui::Order::Foreground, None, Some(bounds))
    }
    /// A search-result preview uses the same menu geometry below the search surface, without input.
    pub fn preview(ctx: &Context, id: Id, anchor: MenuAnchor, items: &[Item]) -> MenuOutput {
        Self::render(ctx, id, anchor, items, false, None, false, egui::Order::Middle, None, None)
    }
    /// Shared menu rows embedded below a form's search field. The form owns outside dismissal.
    /// Pass `just_opened` when the form's opener was activated this frame, so its Enter
    /// does not also pick the first action.
    pub fn items(ui: &mut egui::Ui, id: Id, items: &[Item], just_opened: bool) -> MenuOutput {
        let ctx = ui.ctx().clone();
        let source = state_id(&ctx, id).with("source_layer");
        let frame = ctx.cumulative_frame_nr();
        ctx.data_mut(|data| data.insert_temp(source, (ui.layer_id(), frame)));
        Self::render(&ctx, id, MenuAnchor::Point(ui.cursor().min), items, just_opened, Some(ui), true, egui::Order::Foreground, None, None)
    }
    fn render(
        ctx: &Context,
        id: Id,
        anchor: MenuAnchor,
        items: &[Item],
        just_opened: bool,
        mut inline: Option<&mut egui::Ui>,
        interactive: bool,
        order: egui::Order,
        extra_inside: Option<Rect>,
        bounds: Option<Rect>,
    ) -> MenuOutput {
        let embedded = inline.is_some();
        let viewport = ctx.content_rect();
        let screen = bounds
            .filter(|bounds| bounds.is_finite())
            .map(|bounds| bounds.intersect(viewport))
            .filter(|bounds| bounds.is_positive())
            .unwrap_or(viewport);
        let anchor = match anchor {
            MenuAnchor::Point(point) if point.is_finite() => MenuAnchor::Point(point),
            MenuAnchor::Rect(rect) if rect.is_finite() && rect.is_positive() => MenuAnchor::Rect(rect),
            _ => MenuAnchor::Point(screen.center()),
        };
        let source_key = state_id(ctx, id);
        let key = if interactive { source_key } else { source_key.with("preview") };
        let keyboard_key = active_key(ctx);
        let registry_key = Id::unique(("shared_menus", ctx.viewport_id()));
        let frame = ctx.cumulative_frame_nr();
        let mut state = ctx.data(|data| data.get_temp::<PopupState>(key)).unwrap_or_default();
        if !interactive && let Some(seed) = ctx.data(|data| data.get_temp::<PopupState>(source_key)).filter(|state| state.seeded) {
            state.path = seed.path;
            state.cursor = seed.cursor;
            state.scroll_cursor = seed.scroll_cursor;
            state.seeded = true;
        }
        if just_opened || state.frame.saturating_add(2) < frame {
            let seed = state.seeded.then(|| (state.path.clone(), state.cursor.clone(), state.scroll_cursor));
            state = PopupState::default();
            if let Some((path, cursor, scroll)) = seed {
                state.path = path;
                state.cursor = cursor;
                state.scroll_cursor = scroll;
            }
        }
        let seeded = state.seeded;
        state.source_layer = ctx
            .data(|data| data.get_temp::<(egui::LayerId, u64)>(source_key.with("source_layer")))
            .filter(|(_, seen)| seen.saturating_add(1) >= frame)
            .map(|(layer, _)| layer);
        state.frame = frame;
        state.seeded = false;
        state.open = interactive;
        let mut changed_branch = None;
        if !seeded {
            let mut current = items;
            for (depth, index) in state.path.iter().copied().enumerate() {
                let Some(item) = current.get(index) else {
                    break;
                };
                if state.open_ids.get(depth).is_some_and(|id| *id != item.id) {
                    changed_branch = Some(depth);
                    break;
                }
                current = &item.children;
            }
        }
        if let Some(depth) = changed_branch {
            state.path.truncate(depth);
            state.cursor.truncate(depth);
            state.focus_rows.retain(|(_, level, _)| *level < depth);
        }
        let depth = levels(items, &state.path).len().saturating_sub(1);
        state.path.truncate(depth);
        state.cursor.resize(depth + 1, None);
        let focused = ctx.memory(|memory| memory.focused());
        if !seeded
            && focused != state.last_focus
            && let Some(focus) = focused
            && let Some((_, level, index)) = state.focus_rows.iter().find(|(row, _, _)| *row == focus)
            && let Some(cursor) = state.cursor.get_mut(*level)
        {
            *cursor = Some(*index);
        }
        let auxiliary_focus = focused.is_some_and(|id| state.auxiliary_rows.contains(&id));
        if focused != state.last_focus && focused.is_some_and(|id| state.focus_rows.iter().any(|(row, _, _)| *row == id)) {
            state.keyboard = true;
        }
        state.last_focus = focused;
        let mut output = MenuOutput { open: true, depth, ..Default::default() };
        // Inline actions can precede editors in the same form. The focused
        // editor owns navigation even when it is painted later this frame.
        let keyboard_owner = if interactive && !(embedded && ctx.text_edit_focused()) {
            ctx.data_mut(|data| {
                let current = data.get_temp::<(Id, u64)>(keyboard_key).filter(|(_, seen)| seen.saturating_add(1) >= frame);
                let owner = just_opened || current.is_none_or(|(active, _)| active == id);
                if owner {
                    data.insert_temp(keyboard_key, (id, frame));
                }
                owner
            })
        } else {
            false
        };
        let mut keyboard_close = false;
        if keyboard_owner && (just_opened || changed_branch.is_some()) {
            ctx.input_mut(|input| {
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter);
                input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            });
        }
        let moved = ctx.input(|input| input.pointer.delta() != Vec2::ZERO);
        if moved || ctx.input(|input| input.pointer.any_pressed()) {
            state.keyboard = false;
        }
        if keyboard_owner && !state.editing && !auxiliary_focus && !just_opened && changed_branch.is_none() {
            let left = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft));
            if left && depth == 0 {
                keyboard_close = true;
            } else if left && depth > 0 {
                let previous = state.path.pop();
                state.cursor.truncate(depth);
                if let Some(cursor) = state.cursor.last_mut() {
                    *cursor = previous;
                }
                state.keyboard = true;
                state.scroll_cursor = true;
            } else {
                let current = levels(items, &state.path);
                if let (Some(rows), Some(cursor)) = (current.last(), state.cursor.last_mut()) {
                    let enabled: Vec<_> = rows.iter().take(MAX_ROWS).map(|item| item.enabled && !item.is_sep() && !item.is_heading()).collect();
                    if cursor.is_none() && ctx.input(|input| input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::ArrowRight)) {
                        *cursor = enabled.iter().position(|enabled| *enabled);
                    }
                    let used = ctx.input(|input| {
                        [egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::Home, egui::Key::End].iter().any(|key| input.key_pressed(*key))
                    });
                    let enter = Self::navigate(ctx, &enabled, cursor);
                    let right = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight));
                    if used || enter || right {
                        state.keyboard = true;
                        state.scroll_cursor = true;
                    }
                    if let Some(item) = cursor.and_then(|index| rows.get(index)).filter(|item| item.enabled) {
                        if (!item.children.is_empty() || item.lazy_children)
                            && state.path.len() < MAX_DEPTH
                            && (right || (enter && !item.submenu_action))
                        {
                            if let Some(index) = *cursor {
                                state.path.push(index);
                            }
                            state.cursor.push(item.children.iter().position(|item| item.enabled && !item.is_sep() && !item.is_heading()));
                        } else if enter && item.number.is_none() {
                            output.picked = Some(item.clone());
                        }
                    } else if right {
                        *cursor = rows.iter().position(|item| item.enabled && !item.is_sep() && !item.is_heading());
                    }
                }
            }
        }
        let path = state.path.clone();
        let lists = levels(items, &path);
        let margin = Self::frame().total_margin().sum();
        let root_width = width(ctx, items, screen);
        let root_height = height(items).min((screen.height() - Tokens::get().space_8).max(1.0));
        let (at, root_bottom) = match anchor {
            MenuAnchor::Point(point) => {
                let bottom = (screen.bottom() - Tokens::get().space_4).max(screen.top());
                let usable = Rect::from_min_max(screen.min, pos2(screen.right(), bottom));
                (place(point, Vec2::ZERO, vec2(root_width + margin.x, root_height), usable), bottom)
            }
            MenuAnchor::Rect(rect) => {
                let below_top = rect.bottom().clamp(screen.top(), screen.bottom());
                let above_bottom = rect.top().clamp(screen.top(), screen.bottom());
                let below_bottom = (screen.bottom() - Tokens::get().space_4).max(below_top);
                let below = (below_bottom - below_top).max(0.0);
                let above = (above_bottom - screen.top()).max(0.0);
                let full = height(items);
                let open_below = full <= below || (full > above && below >= above);
                let minimum = full.min(Tokens::get().control_height + Tokens::get().space_2 + margin.y);
                let (y, bottom) = if below.max(above) < minimum {
                    // An exceptionally small host may have no side that fits one action.
                    // Use its whole capacity so the selected row remains hittable.
                    (screen.top(), screen.bottom() - Tokens::get().space_4)
                } else if open_below {
                    (below_top, below_bottom)
                } else {
                    (above_bottom - full.min(above), above_bottom)
                };
                (pos2(rect.left().min(screen.right() - root_width - margin.x).max(screen.left()), y), bottom)
            }
        };
        let pointer = ctx.pointer_hover_pos();
        if state.pointer_gate.is_some_and(|(_, anchor)| pointer.is_none_or(|point| !anchor.contains(point))) {
            state.pointer_gate = None;
        }
        let released = ctx.input(|input| input.pointer.button_released(egui::PointerButton::Primary));
        output.inside = extra_inside.is_some_and(|rect| pointer.is_some_and(|point| rect.contains(point)))
            || match anchor {
                MenuAnchor::Rect(rect) => pointer.is_some_and(|point| rect.contains(point)),
                _ => false,
            };
        let mut seen = Vec::new();
        state.visible_rects.clear();
        let mut previous_anchor = None;
        let mut leftward = false;
        let mut editing = false;
        for (depth, rows) in lists.iter().enumerate() {
            let picked_before = output.picked.clone();
            let content_width = width(ctx, rows, screen);
            let estimated = vec2(content_width + margin.x, height(rows).min(screen.height()));
            let position = if depth == 0 {
                at
            } else {
                let Some(rect) = previous_anchor else { break };
                Self::flyout_at(screen, rect, estimated, leftward)
            };
            let area_id = key.with(("level", depth));
            let cursor = state.cursor.get(depth).copied().flatten();
            let mut body = |ui: &mut egui::Ui| {
                if !interactive {
                    ui.disable();
                }
                ui.set_width(content_width);
                ui.spacing_mut().item_spacing.y = 0.0;
                let bottom = if depth == 0 { root_bottom } else { screen.bottom() - Tokens::get().space_4 };
                let room = (bottom - position.y - margin.y).max(0.0);
                let reveal = state.scroll_cursor.then_some(cursor).flatten().filter(|index| *index < rows.len()).map(|index| {
                    let top = height(&rows[..index]) - margin.y;
                    (top, top + height(&rows[index..=index]) - margin.y)
                });
                super::scroll_rows_reveal(ui, area_id.with("rows"), room, height(rows) - margin.y, reveal, |ui| {
                    ui.set_width(content_width);
                    draw_level(
                        ui,
                        rows,
                        depth,
                        cursor,
                        path.get(depth).copied(),
                        state.leaving.is_some_and(|(level, _)| level == depth),
                        &mut output,
                        &mut editing,
                        state.scroll_cursor && keyboard_owner,
                        state.keyboard,
                        depth + 1 == lists.len(),
                    )
                })
            };
            let response = if depth == 0 && inline.is_some() {
                let Some(ui) = inline.as_deref_mut() else {
                    break;
                };
                let start = ui.cursor().min;
                let inner = body(ui);
                let rect = Rect::from_min_max(start, ui.min_rect().max);
                let response = ui.interact(rect, area_id, Sense::hover());
                egui::InnerResponse { inner, response }
            } else {
                Container::new(ctx, Surface::Popup)
                    .area(area_id)
                    .kind(egui::UiKind::Menu)
                    .fixed_pos(position)
                    .constrain(true)
                    .order(order)
                    .interactable(interactive)
                    .show(ctx, |ui| {
                        ui.set_width(content_width + margin.x);
                        Self::show_frame(ui, Self::frame(), body).inner
                    })
            };
            if !response.inner.sizing {
                let layer = response.response.layer_id;
                let owner_key = layer.id.with(("shared_menu_owner", ctx.viewport_id()));
                ctx.data_mut(|data| data.insert_temp(owner_key, (id, layer, frame)));
                state.visible_rects.push(response.response.interact_rect);
            }
            if depth == 0 {
                output.sizing = response.inner.sizing;
                output.rect = Some(response.response.rect);
            }
            if let Some(parent) = previous_anchor {
                leftward = response.response.rect.center().x < parent.center().x;
            }
            output.inside |= response.response.contains_pointer() || pointer.is_some_and(|point| response.response.rect.contains(point));
            previous_anchor = response.inner.anchor;
            // On a narrow viewport the new flyout can cover its own opener.
            // Finishing the opener click must not activate the child underneath.
            if released && state.pointer_gate.is_some_and(|(level, anchor)| depth >= level && pointer.is_some_and(|point| anchor.contains(point))) {
                output.picked = picked_before;
            }
            seen.push(response.inner);
        }
        state.editing = editing;
        state.auxiliary_rows = seen.iter().flat_map(|seen| seen.auxiliary.iter().copied()).collect();
        state.focus_rows = seen.iter().enumerate().flat_map(|(depth, seen)| seen.ids.iter().map(move |(id, index)| (*id, depth, *index))).collect();
        // New areas measure while disabled. Transfer focus and scroll once the
        // flyout is visible, rather than losing the request in its sizing pass.
        state.scroll_cursor &= seen.iter().any(|seen| seen.sizing);
        if interactive && !state.keyboard {
            if let Some((depth, index)) = seen.iter().enumerate().rev().find_map(|(depth, seen)| seen.opens.map(|index| (depth, index))) {
                if path.get(depth) != Some(&index) && depth < MAX_DEPTH {
                    state.path.truncate(depth);
                    state.path.push(index);
                    state.cursor.truncate(depth + 1);
                    state.cursor.push(None);
                    state.pointer_gate = seen.get(depth).and_then(|seen| seen.opens_anchor).map(|anchor| (depth + 1, anchor));
                    ctx.request_repaint();
                }
                state.leaving = None;
            } else if let Some(depth) = seen.iter().rposition(|seen| seen.other_row).filter(|depth| *depth < path.len()) {
                let mut since = state.leaving.filter(|(level, _)| *level == depth).map(|(_, since)| since);
                if moved || since.is_some() {
                    if Self::flyout_leaving(ctx, &mut since) {
                        state.path.truncate(depth);
                        state.cursor.truncate(depth + 1);
                        ctx.request_repaint();
                    }
                    state.leaving = since.map(|since| (depth, since));
                }
            } else {
                state.leaving = None;
            }
        }
        if released {
            state.pointer_gate = None;
        }
        let outside = interactive && !embedded && !just_opened && ctx.input(|input| input.pointer.any_pressed()) && !output.inside;
        let escape = keyboard_owner && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        // Control-channel windows may start in the background. Dismiss when an
        // already open menu actually loses focus, rather than forbidding background use.
        let unfocused =
            interactive && !just_opened && ctx.input(|input| input.events.iter().any(|event| matches!(event, egui::Event::WindowFocused(false))));
        if escape || keyboard_close {
            // Cancellation wins if activation and dismissal arrive in one input frame.
            output.picked = None;
        }
        if !interactive {
            output.picked = None;
            output.hovered = None;
        }
        output.close = keyboard_close || outside || escape || unfocused || output.picked.as_ref().is_some_and(|item| !item.keep_open);
        output.open = !output.close;
        output.depth = state.path.len();
        state.open = output.open && interactive;
        state.open_ids.clear();
        let mut current = items;
        for &index in &state.path {
            let Some(item) = current.get(index) else {
                break;
            };
            state.open_ids.push(item.id.clone());
            current = &item.children;
        }
        let hot_registry = Id::unique(("shared_menu_hot_rows", ctx.viewport_id()));
        let hot_pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|data| {
            let hot = data.get_temp_mut_or_default::<Vec<(Id, u64)>>(hot_registry);
            hot.retain(|(_, seen)| *seen == hot_pass);
            if interactive && let Some(row) = output.active_row {
                hot.push((row, hot_pass));
            }
            data.insert_temp(key, state);
            if output.close && data.get_temp::<(Id, u64)>(keyboard_key).is_some_and(|(active, _)| active == id) {
                data.remove::<(Id, u64)>(keyboard_key);
            }
            if !interactive {
                return;
            }
            let entries = data.get_temp_mut_or_default::<Vec<(Id, u64, bool)>>(registry_key);
            entries.retain(|(menu, seen, _)| *menu != id && seen.saturating_add(1) >= frame);
            entries.push((id, frame, output.open && interactive));
        });
        if output.close {
            ctx.request_repaint();
        }
        output
    }
}
