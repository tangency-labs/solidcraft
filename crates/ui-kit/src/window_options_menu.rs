//! Pop-out chrome controls. The host owns the window state and applies each returned action.

use crate::choice::{Segments, segmented};
use crate::menu::{Menu, MenuItem};
use crate::{Container, Surface};
use egui::{Id, Key, Modifiers, Pos2, Rect, Response, Ui, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_ui_text::tr;

const WIDTH: f32 = 360.0;
const ROW_HEIGHT: f32 = 34.0;
const MENU_HEIGHT: f32 = 520.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardHeaders {
    Always,
    Hover,
    Hidden,
}

impl CardHeaders {
    const ALL: [Self; 3] = [Self::Always, Self::Hover, Self::Hidden];

    fn index(self) -> usize {
        match self {
            Self::Always => 0,
            Self::Hover => 1,
            Self::Hidden => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopBar {
    Always,
    Edge,
}

pub struct DisplayChoice<'a> {
    pub id: usize,
    pub name: &'a str,
    pub current: bool,
}

pub struct WindowMenu<'a> {
    pub title: &'a str,
    pub displays: &'a [DisplayChoice<'a>],
    pub card_headers: CardHeaders,
    pub keep_on_top: bool,
    pub panels: bool,
    pub ribbon: bool,
    pub ribbon_auto_hide: bool,
    pub top_bar: TopBar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    CardHeaders(CardHeaders),
    KeepOnTop(bool),
    Panels(bool),
    Ribbon(bool),
    RibbonAutoHide(bool),
    TopBar(TopBar),
    MoveToDisplay(usize),
    Rename,
    UseAsDefault,
    ReturnViews,
    Close,
}

#[derive(Clone, Copy, Default)]
struct State {
    open: bool,
    focus: bool,
    displays: bool,
    focus_display: bool,
    scroll_focus: Option<Id>,
}

struct FocusRows {
    ids: Vec<Id>,
    scroll_focus: Option<Id>,
}

impl FocusRows {
    fn new(scroll_focus: Option<Id>) -> Self {
        Self { ids: Vec::new(), scroll_focus }
    }

    fn step(&self, ui: &mut Ui, first: &mut bool) -> Option<Id> {
        if self.ids.is_empty() || !ui.is_visible() || ui.is_sizing_pass() {
            return self.scroll_focus;
        }
        let at = self.ids.iter().position(|id| ui.memory(|memory| memory.has_focus(*id)));
        let direction = ui.input_mut(|input| {
            if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                1
            } else if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                -1
            } else {
                0
            }
        });
        let len = self.ids.len();
        let index = match (direction, at) {
            (1, Some(index)) => (index + 1) % len,
            (-1, Some(index)) => (index + len - 1) % len,
            (-1, None) => len - 1,
            (1, None) => 0,
            _ if *first => 0,
            _ => return None,
        };
        *first = false;
        let id = self.ids[index];
        ui.memory_mut(|memory| {
            memory.move_focus(egui::FocusDirection::None);
            memory.request_focus(id);
        });
        Some(id)
    }

    fn row(&mut self, ui: &Ui, response: &Response) {
        if response.enabled() {
            self.register(ui, response.id, response.rect);
        }
    }

    fn register(&mut self, ui: &Ui, id: Id, rect: Rect) {
        if self.scroll_focus == Some(id) {
            ui.scroll_to_rect(rect, None);
        }
        self.ids.push(id);
    }
}

/// Whether the window menu is holding the pop-out chrome open.
pub fn is_open(ctx: &egui::Context, anchor: Id) -> bool {
    ctx.data(|data| data.get_temp::<State>(anchor.with("window_options"))).is_some_and(|state| state.open)
}

fn popup_origin(screen: Rect, anchor: Rect, width: f32, height: f32) -> Pos2 {
    let x = (anchor.right() - width).max(screen.left()).min(screen.right() - width);
    let y = if anchor.bottom() + height <= screen.bottom() {
        anchor.bottom()
    } else if anchor.top() - height >= screen.top() {
        anchor.top() - height
    } else {
        (screen.bottom() - height).max(screen.top())
    };
    pos2(x, y)
}

/// Paint the options popover anchored to the title bar. A context click can open it at the same
/// anchor even when it did not hit the overflow button.
pub fn show(ui: &mut Ui, anchor: Response, context_open: bool, menu: &WindowMenu<'_>) -> Option<Action> {
    let ctx = ui.ctx().clone();
    let id = anchor.id.with("window_options");
    let mut state = ctx.data(|data| data.get_temp::<State>(id)).unwrap_or_default();
    let opened_now = context_open || (anchor.clicked() && !state.open);
    if opened_now {
        state = State { open: true, focus: true, ..State::default() };
    } else if anchor.clicked() {
        state = State::default();
        anchor.request_focus();
    }
    ctx.accesskit_node_builder(anchor.id, |node| node.set_expanded(state.open));
    let mut action = None;
    if state.open {
        let screen = ctx.content_rect().shrink(8.0);
        let width = WIDTH.min(screen.width());
        let height = screen.height().min(MENU_HEIGHT);
        if width > 0.0 && height > 0.0 {
            let at = popup_origin(screen, anchor.rect, width, height);
            let popup = Container::new(&ctx, Surface::Popup)
                .area(id.with("popup"))
                .default_size(vec2(width, height))
                .fixed_pos(at)
                .order(egui::Order::Foreground)
                .show(&ctx, |ui| {
                    ui.set_max_height(height);
                    Menu::show(ui, width, |ui| contents(ui, id, menu, height, &mut state))
                });
            action = popup.inner.0;
            let outside = ctx.input(|input| {
                input.pointer.any_pressed()
                    && input.pointer.interact_pos().is_some_and(|at| {
                        !popup.response.rect.contains(at) && !anchor.rect.contains(at) && !popup.inner.1.is_some_and(|rect| rect.contains(at))
                    })
            });
            let escape = ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
            if (!opened_now && outside) || escape || matches!(action, Some(Action::Rename | Action::ReturnViews | Action::Close)) {
                state = State::default();
                anchor.request_focus();
            }
        }
    }
    ctx.data_mut(|data| data.insert_temp(id, state));
    action
}

fn contents(ui: &mut Ui, id: Id, menu: &WindowMenu<'_>, height: f32, state: &mut State) -> (Option<Action>, Option<Rect>) {
    let t = Tokens::get();
    let display = menu.displays.iter().find(|display| display.current);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
        crate::icons::paint_ink(ui.painter(), icon, crate::icons::window_icon(true), t.text_dim);
        ui.add(egui::Label::new(egui::RichText::new(menu.title).font(crate::type_face::semibold(13.0)).color(t.text)).truncate());
        if let Some(display) = display {
            ui.add(egui::Label::new(egui::RichText::new(display.name).size(12.0).color(t.text_dim)).truncate());
        }
    });
    ui.add_space(6.0);
    let preview_height = (height - 26.0 - 2.0 * ROW_HEIGHT - 80.0).clamp(0.0, 208.0);
    crate::window_preview::show(ui, menu, preview_height);
    let middle_height = (height - ui.min_rect().height() - 2.0 * ROW_HEIGHT - 26.0).max(0.0);
    let mut focus = FocusRows::new(state.scroll_focus);
    let (mut action, flyout) = egui::ScrollArea::vertical()
        .id_salt(id.with("options_scroll"))
        .max_height(middle_height)
        .auto_shrink([false, true])
        .show(ui, |ui| middle(ui, id, menu, state, &mut focus))
        .inner;
    Menu::separator(ui);
    for (key, label, icon, shortcut, choice) in [
        ("return", "multi_window.return_views", "undo", "", Action::ReturnViews),
        ("close", "multi_window.close_window", "close", "Ctrl+W", Action::Close),
    ] {
        let response = row(ui, key, tr(label), icon, shortcut);
        focus.row(ui, &response);
        if response.clicked() {
            action = Some(choice);
        }
    }
    if !state.displays {
        state.scroll_focus = focus.step(ui, &mut state.focus);
    }
    (action, flyout)
}

fn middle(ui: &mut Ui, id: Id, menu: &WindowMenu<'_>, state: &mut State, focus: &mut FocusRows) -> (Option<Action>, Option<Rect>) {
    Menu::heading(ui, tr("window_menu.show_here"));
    let mut action = ribbon_options(ui, menu, focus);
    action = top_bar(ui, menu.top_bar, focus).or(action);
    action = panel_options(ui, menu.panels, focus).or(action);
    action = card_headers(ui, menu.card_headers, focus).or(action);
    Menu::separator(ui);
    Menu::heading(ui, tr("window_menu.window"));
    let mut on = menu.keep_on_top;
    let key = tr("window_options.keep_on_top");
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        ui.label(key);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            let response = crate::preferences::theme_option_switch(ui, key, &mut on);
            solidcraft_ui_registry::publish_target("window_options:keep_on_top", response.rect);
            focus.row(ui, &response);
        });
    });
    if on != menu.keep_on_top {
        action = Some(Action::KeepOnTop(on));
    }

    let (display_action, flyout) = displays(ui, id, menu.displays, state, focus);
    action = display_action.or(action);
    for (key, label, icon, shortcut, choice) in [
        ("rename", "window_menu.rename", "rename", "F2", Action::Rename),
        ("use_defaults", "window_menu.use_default", "star", "", Action::UseAsDefault),
    ] {
        let response = row(ui, key, tr(label), icon, shortcut);
        focus.row(ui, &response);
        if response.clicked() {
            action = Some(choice);
        }
    }
    (action, flyout)
}

fn ribbon_options(ui: &mut Ui, menu: &WindowMenu<'_>, focus: &mut FocusRows) -> Option<Action> {
    use crate::BackingTooltipExt as _;
    let mut ribbon = menu.ribbon;
    let mut auto_hide = menu.ribbon_auto_hide;
    let t = Tokens::get();
    let label = tr("window_ribbon.ribbon");
    ui.horizontal(|ui| {
        ui.add_space(t.space_8);
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(t.space_8);
            let response = crate::preferences::theme_option_switch(ui, label, &mut ribbon);
            solidcraft_ui_registry::publish_target("window_options:ribbon", response.rect);
            focus.row(ui, &response);
        });
    });
    let label = tr("window_ribbon.auto_hide");
    let available = menu.ribbon;
    let name = if available { label } else { tr("window_ribbon.auto_hide_unavailable") };
    ui.add_enabled_ui(available, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(t.space_8 + 22.0);
            ui.label(label).backing_on_disabled_hover_text(name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(t.space_8);
                let response = crate::preferences::theme_option_switch(ui, name, &mut auto_hide);
                solidcraft_ui_registry::publish_target("window_options:ribbon_auto_hide", response.rect);
                focus.row(ui, &response);
            });
        });
    });
    if ribbon != menu.ribbon {
        Some(Action::Ribbon(ribbon))
    } else if auto_hide != menu.ribbon_auto_hide {
        Some(Action::RibbonAutoHide(auto_hide))
    } else {
        None
    }
}

fn panel_options(ui: &mut Ui, current: bool, focus: &mut FocusRows) -> Option<Action> {
    use crate::BackingTooltipExt as _;
    let t = Tokens::get();
    let label = tr("window_panels.other_panels");
    let description = tr("window_panels.workspace_panels");
    let mut panels = current;
    ui.horizontal(|ui| {
        ui.add_space(t.space_8);
        ui.label(label).backing_on_hover_text(description);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(t.space_8);
            let response = crate::preferences::theme_option_switch(ui, label, &mut panels).backing_on_hover_text(description);
            solidcraft_ui_registry::publish_target("window_options:other_panels", response.rect);
            focus.row(ui, &response);
        });
    });
    (panels != current).then_some(Action::Panels(panels))
}

fn top_bar(ui: &mut Ui, mode: TopBar, focus: &mut FocusRows) -> Option<Action> {
    let labels = [tr("window_options.always"), tr("window_topbar.edge")];
    let targets = ["window_options:top_bar:always", "window_options:top_bar:edge"];
    let selected = usize::from(mode == TopBar::Edge);
    segment_row(ui, tr("window_topbar.top_bar"), selected, &labels, &targets, 170.0, focus)
        .map(|index| Action::TopBar(if index == 0 { TopBar::Always } else { TopBar::Edge }))
}

fn card_headers(ui: &mut Ui, headers: CardHeaders, focus: &mut FocusRows) -> Option<Action> {
    let labels = [tr("window_options.always"), tr("window_options.on_hover"), tr("window_options.hidden")];
    let targets = ["window_options:headers:always", "window_options:headers:hover", "window_options:headers:hidden"];
    segment_row(ui, tr("window_options.card_headers"), headers.index(), &labels, &targets, 210.0, focus)
        .and_then(|index| CardHeaders::ALL.get(index).copied())
        .map(Action::CardHeaders)
}

fn segment_row(
    ui: &mut Ui,
    label: &str,
    mut selected: usize,
    labels: &[&str],
    targets: &[&str],
    max_width: f32,
    focus: &mut FocusRows,
) -> Option<usize> {
    let mut changed = false;
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let width = ui.available_width().min(max_width);
                let t = Tokens::get();
                ui.allocate_ui_with_layout(vec2(width, ROW_HEIGHT), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // A segment has the usual native focus target. Register the selected one as the
                    // row's default target and preserve the active segment when navigating from it.
                    let ids = (0..labels.len()).map(|index| ui.make_persistent_id(("segment", index))).collect::<Vec<_>>();
                    let focused = ids.iter().position(|id| ui.memory(|memory| memory.has_focus(*id)));
                    if focused.is_some() {
                        let step = ui.input_mut(|input| {
                            if input.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                                -1
                            } else if input.consume_key(Modifiers::NONE, Key::ArrowRight) {
                                1
                            } else {
                                0
                            }
                        });
                        if step != 0 {
                            selected = (focused.unwrap_or(selected) + (step + labels.len() as isize) as usize) % labels.len();
                            if let Some(id) = ids.get(selected) {
                                ui.memory_mut(|memory| {
                                    memory.move_focus(egui::FocusDirection::None);
                                    memory.request_focus(*id);
                                });
                            }
                            changed = true;
                        }
                    }
                    let rect = Rect::from_min_size(ui.next_widget_position(), vec2(width, t.control_height));
                    changed |= segmented(ui, &mut selected, labels, Segments { compact: true, small: true, targets, ..Default::default() });
                    if let Some(id) = ids.get(focused.unwrap_or(selected)) {
                        focus.register(ui, *id, rect);
                    }
                });
            });
        })
    });
    changed.then_some(selected)
}

fn displays(ui: &mut Ui, id: Id, displays: &[DisplayChoice<'_>], state: &mut State, focus: &mut FocusRows) -> (Option<Action>, Option<Rect>) {
    let mut row = MenuItem::new("window_options:display", tr("window_menu.move_to_display"), "display");
    row.shortcut = displays.iter().find(|display| display.current).map_or("", |display| display.name);
    row.submenu = true;
    row.enabled = !displays.is_empty();
    row.highlighted = state.displays;
    let response = row.show(ui);
    focus.row(ui, &response);
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_expanded(state.displays));
    let right = response.has_focus() && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowRight));
    if response.clicked() || right {
        state.displays = !state.displays || right;
        state.focus_display = true;
    }
    if !state.displays {
        return (None, None);
    }
    let ctx = ui.ctx().clone();
    let height = ctx.content_rect().height() - 16.0;
    let flyout = Menu::submenu(&ctx, id.with("displays"), response.rect, (displays.len() as f32 * ROW_HEIGHT).min(height), false, |ui| {
        let mut focus = FocusRows::new(state.scroll_focus);
        let (action, scroll_focus) = egui::ScrollArea::vertical()
            .max_height((height - 14.0).max(0.0))
            .show(ui, |ui| {
                let mut action = None;
                for display in displays {
                    let display_id = format!("window_options:display:{}", display.id);
                    let mut row = MenuItem::new(&display_id, display.name, "");
                    row.checked = Some(display.current);
                    let response = row.show(ui);
                    focus.row(ui, &response);
                    if response.clicked() {
                        action = Some(Action::MoveToDisplay(display.id));
                    }
                }
                (action, focus.step(ui, &mut state.focus_display))
            })
            .inner;
        state.scroll_focus = scroll_focus;
        action
    });
    let back = ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape) || input.consume_key(Modifiers::NONE, Key::ArrowLeft));
    if back || flyout.inner.is_some() {
        state.displays = false;
        response.request_focus();
    }
    (flyout.inner, Some(flyout.response.rect))
}

fn row(ui: &mut Ui, id: &str, label: &str, icon: &str, shortcut: &str) -> Response {
    let id = format!("window_options:{id}");
    let mut row = MenuItem::new(&id, label, icon);
    row.shortcut = shortcut;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), egui::Sense::click());
    row.paint(ui, rect, &response, 0.0);
    response
}
