//! Pop-out chrome controls. The host owns the window state and applies each returned action.

use crate::menu::Menu;
use egui::{Id, Response, Ui};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardHeaders {
    Always,
    Hover,
    Hidden,
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

/// The menu's words, translated by the host.
#[derive(Clone, Debug, Default)]
pub struct WindowText {
    pub preview: String,
    pub show_here: String,
    pub window: String,
    pub keep_on_top: String,
    pub ribbon: String,
    pub auto_hide: String,
    pub auto_hide_unavailable: String,
    pub other_panels: String,
    pub workspace_panels: String,
    pub top_bar: String,
    pub always: String,
    pub edge: String,
    pub card_headers: String,
    pub on_hover: String,
    pub hidden: String,
    pub move_to_display: String,
    pub interface_size: String,
    pub rename: String,
    pub use_default: String,
    pub return_views: String,
    pub close_window: String,
}

pub struct WindowMenu<'a> {
    pub text: &'a WindowText,
    pub title: &'a str,
    pub displays: &'a [DisplayChoice<'a>],
    pub card_headers: CardHeaders,
    pub keep_on_top: bool,
    pub interface_sizes: &'a [u16],
    pub interface_size: u16,
    pub panels: bool,
    pub ribbon: bool,
    pub ribbon_auto_hide: bool,
    pub top_bar: TopBar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    CardHeaders(CardHeaders),
    KeepOnTop(bool),
    InterfaceSize(u16),
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
}

/// Whether the window's shared command menu is visible.
pub fn is_open(ctx: &egui::Context, anchor: Id) -> bool {
    ctx.data(|data| data.get_temp::<State>(anchor.with("window_options"))).is_some_and(|state| state.open)
}

fn entry(id: &str, label: &str, icon: &str) -> crate::Item {
    let id = format!("window_options:{id}");
    let mut item = crate::Item::action(&id, label, icon);
    item.aliases.push(id);
    item
}

fn toggle(id: &str, label: &str, value: bool) -> crate::Item {
    let mut item = entry(id, label, "");
    item.checked = Some(value);
    item.keep_open = true;
    item
}

/// Only data describes this menu; the shared component owns all interaction and layout.
fn definition(menu: &WindowMenu<'_>) -> Vec<crate::Item> {
    let text = menu.text;
    let mut auto_hide = toggle("ribbon_auto_hide", &text.auto_hide, menu.ribbon_auto_hide);
    auto_hide.enabled = menu.ribbon;
    auto_hide.tip = (!menu.ribbon).then(|| text.auto_hide_unavailable.clone());
    auto_hide.indent = 1;
    let top_modes = [("always", &text.always, TopBar::Always), ("edge", &text.edge, TopBar::Edge)]
        .into_iter()
        .map(|(id, label, mode)| {
            let mut item = toggle(&format!("top_bar:{id}"), label, menu.top_bar == mode);
            item.radio = true;
            item
        })
        .collect();
    let header_modes =
        [("always", &text.always, CardHeaders::Always), ("hover", &text.on_hover, CardHeaders::Hover), ("hidden", &text.hidden, CardHeaders::Hidden)]
            .into_iter()
            .map(|(id, label, mode)| {
                let mut item = toggle(&format!("headers:{id}"), label, menu.card_headers == mode);
                item.radio = true;
                item
            })
            .collect();
    let sizes = menu
        .interface_sizes
        .iter()
        .map(|percent| {
            let mut item = toggle(&format!("size:{percent}"), &format!("{percent}%"), *percent == menu.interface_size);
            item.radio = true;
            item
        })
        .collect();
    let displays = menu
        .displays
        .iter()
        .map(|display| {
            let mut item = toggle(&format!("display:{}", display.id), display.name, display.current);
            item.radio = true;
            item.keep_open = false;
            item
        })
        .collect();
    let mut panels = toggle("other_panels", &text.other_panels, menu.panels);
    panels.tip = Some(text.workspace_panels.clone());
    let mut size = crate::Item::submenu("window_options:interface_size", &text.interface_size, "zoom", sizes);
    size.shortcut = format!("{}%", menu.interface_size);
    size.aliases.push(size.id.clone());
    let mut display = crate::Item::submenu("window_options:display", &text.move_to_display, "display", displays);
    display.enabled = !menu.displays.is_empty();
    display.shortcut = menu.displays.iter().find(|display| display.current).map_or(String::new(), |display| display.name.to_owned());
    display.aliases.push(display.id.clone());
    vec![
        crate::Item::heading(menu.title),
        crate::Item::heading(&text.show_here),
        toggle("ribbon", &text.ribbon, menu.ribbon),
        auto_hide,
        crate::Item::submenu("window_options:top_bar", &text.top_bar, "", top_modes),
        panels,
        crate::Item::submenu("window_options:headers", &text.card_headers, "", header_modes),
        crate::Item::sep(),
        crate::Item::heading(&text.window),
        toggle("keep_on_top", &text.keep_on_top, menu.keep_on_top),
        size,
        display,
        entry("rename", &text.rename, "rename").key("F2"),
        entry("use_defaults", &text.use_default, "star"),
        crate::Item::sep(),
        entry("return", &text.return_views, "undo"),
        entry("close", &text.close_window, "close").key("Ctrl+W"),
    ]
}

fn choice(id: &str, menu: &WindowMenu<'_>) -> Option<Action> {
    let id = id.strip_prefix("window_options:")?;
    Some(match id {
        "ribbon" => Action::Ribbon(!menu.ribbon),
        "ribbon_auto_hide" => Action::RibbonAutoHide(!menu.ribbon_auto_hide),
        "other_panels" => Action::Panels(!menu.panels),
        "keep_on_top" => Action::KeepOnTop(!menu.keep_on_top),
        "top_bar:always" => Action::TopBar(TopBar::Always),
        "top_bar:edge" => Action::TopBar(TopBar::Edge),
        "headers:always" => Action::CardHeaders(CardHeaders::Always),
        "headers:hover" => Action::CardHeaders(CardHeaders::Hover),
        "headers:hidden" => Action::CardHeaders(CardHeaders::Hidden),
        "rename" => Action::Rename,
        "use_defaults" => Action::UseAsDefault,
        "return" => Action::ReturnViews,
        "close" => Action::Close,
        other => {
            if let Some(percent) = other.strip_prefix("size:") {
                Action::InterfaceSize(percent.parse().ok()?)
            } else {
                let display = other.strip_prefix("display:")?;
                Action::MoveToDisplay(display.parse().ok()?)
            }
        }
    })
}

/// Opening adapters differ; every window option is drawn by the same shared menu.
pub fn show(ui: &mut Ui, anchor: Response, context_open: bool, menu: &WindowMenu<'_>) -> Option<Action> {
    let ctx = ui.ctx();
    let id = anchor.id.with("window_options");
    let mut state = ctx.data(|data| data.get_temp::<State>(id)).unwrap_or_default();
    let just_opened = context_open || (anchor.clicked() && !state.open);
    if context_open || anchor.clicked() {
        state.open = context_open || !state.open;
    }
    let mut picked = None;
    if state.open {
        let out = Menu::popup(ctx, id, crate::MenuAnchor::Rect(anchor.rect), &definition(menu), just_opened);
        picked = out.picked.and_then(|item| choice(&item.id, menu));
        if out.close {
            state.open = false;
            anchor.request_focus();
        }
    }
    ctx.accesskit_node_builder(anchor.id, |node| node.set_expanded(state.open));
    ctx.data_mut(|data| data.insert_temp(id, state));
    picked
}
