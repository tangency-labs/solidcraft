//! The view strip's per-window layout menu. The app bar owns the popup and its dismissal.

use crate::app_bar::{TabStrip, paint_layout_glyph, paint_view_glyph};
use egui::Color32;
use solidcraft_theme::Tokens;
use solidcraft_ui_text::tr_label;

/// The menu's words, read by the host from its message catalog. The two templates hold `{layout}` and
/// `{count}`, and `{design}`, for the kit to fill in.
#[derive(Default)]
pub struct LayoutText {
    pub heading: String,
    pub summary_single: String,
    pub summary: String,
    pub this_window: String,
    pub show: String,
    pub bring: String,
    pub windows: String,
    pub link: String,
    pub off: String,
    pub orientation: String,
    pub zoom: String,
    pub both: String,
    pub new_cells: String,
    pub detach: String,
    pub equal: String,
    pub reset: String,
    pub clean: String,
    pub save: String,
    pub saved: String,
    pub manage: String,
}

pub struct LayoutMenu {
    pub text: LayoutText,
    pub design: String,
    pub window: u32,
    pub native: bool,
    pub visible: usize,
    pub link: String,
    pub can_link: bool,
    pub clean: bool,
    pub windows: Vec<LayoutWindow>,
    pub saved: Vec<String>,
    pub matching_saved: Option<String>,
}

pub struct LayoutWindow {
    pub id: u32,
    pub name: String,
    pub display: Option<String>,
    pub views: Vec<LayoutView>,
}

pub struct LayoutView {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub accent: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sections {
    pub(crate) equal: bool,
    pub(crate) windows: bool,
    pub(crate) link: bool,
    pub(crate) detach: bool,
}

pub(crate) fn sections(data: &LayoutMenu) -> Sections {
    let windows = data.windows.len() > 1;
    Sections { equal: data.visible >= 2, windows, link: data.visible >= 2, detach: data.native && !windows }
}

pub(crate) fn keep_open(action: &str) -> bool {
    action.starts_with("win.focus.")
        || action.starts_with("win.bring.")
        || action.starts_with("fill.")
        || action.starts_with("layout.link.")
        || matches!(action, "layout.single" | "layout.side_by_side" | "layout.stacked" | "layout.large_and_two" | "layout.grid")
}

fn item(id: &str, label: &str, icon: &str) -> crate::Item {
    let mut item = crate::Item::action(id, label, icon);
    item.keep_open = keep_open(id);
    item.aliases.push(id.to_owned());
    item
}

fn radio(id: &str, label: &str, icon: &str, selected: bool) -> crate::Item {
    let mut item = item(id, label, icon);
    item.checked = Some(selected);
    item.radio = true;
    item
}

/// The view layout switcher is a normal shared command menu, described only by rows.
pub(crate) fn definition(strip: &TabStrip<'_>, data: &LayoutMenu) -> Vec<crate::Item> {
    let flags = sections(data);
    let mut rows = vec![crate::Item::heading(&data.text.heading)];
    for row in strip.layout_menu.iter().filter(|row| !row.is_heading() && row.id.starts_with("layout.")) {
        let mut entry = radio(&row.id, tr_label(&row.label), "", row.id == strip.layout_current);
        let layout = row.id.to_owned();
        entry.icon = crate::ItemIcon::paint(move |p, r, opacity| {
            paint_layout_glyph(p, r, &layout, Tokens::get().text_dim.gamma_multiply(opacity));
        });
        rows.push(entry);
    }
    if flags.equal {
        rows.push(item("layout.equal", &data.text.equal, ""));
    }
    let fills = strip
        .layout_menu
        .iter()
        .filter(|row| row.id.starts_with("fill."))
        .map(|row| radio(&row.id, tr_label(&row.label), "", row.id == strip.layout_fill))
        .collect();
    rows.push(crate::Item::submenu("layout.fill", &data.text.new_cells, "", fills));
    if flags.windows {
        rows.push(crate::Item::sep());
        rows.push(crate::Item::heading(&data.text.windows.replace("{design}", &data.design)));
        for window in
            data.windows.iter().filter(|window| window.id == data.window).chain(data.windows.iter().filter(|window| window.id != data.window))
        {
            let mut children = Vec::new();
            if window.id != data.window {
                children.push(item(&format!("win.focus.{}", window.id), &data.text.show, ""));
                children.push(item(&format!("win.bring.{}", window.id), &data.text.bring, "").on_if(window.id != 0));
            }
            children.extend(window.views.iter().map(|view| crate::Item::heading(&view.name)));
            let mut name = if window.id == data.window && !data.text.this_window.is_empty() {
                format!("{} — {}", data.text.this_window, window.name)
            } else {
                window.name.clone()
            };
            if let Some(display) = window.display.as_ref().filter(|display| !display.is_empty()) {
                name.push_str(" · ");
                name.push_str(display);
            }
            let mut window_menu =
                crate::Item::submenu(&format!("layout.window.{}", window.id), &name, crate::icons::window_icon(window.id == data.window), children);
            window_menu.tip = Some(name);
            // A current window's child list is informative, but still opens for reading its views.
            window_menu.enabled = !window_menu.children.is_empty();
            rows.push(window_menu);
        }
    }
    if flags.link {
        rows.push(crate::Item::sep());
        let children = [("off", &data.text.off), ("orientation", &data.text.orientation), ("zoom", &data.text.zoom), ("both", &data.text.both)]
            .into_iter()
            .map(|(mode, label)| radio(&format!("layout.link.{mode}"), label, "", data.link == mode).on_if(data.can_link))
            .collect();
        let current = match data.link.as_str() {
            "orientation" => &data.text.orientation,
            "zoom" => &data.text.zoom,
            "both" => &data.text.both,
            _ => &data.text.off,
        };
        let mut link = crate::Item::submenu("layout.link", &data.text.link, "", children).key(current);
        // The choices remain inspectable when linking cannot currently be changed.
        link.enabled = true;
        rows.push(link);
    }
    rows.push(crate::Item::sep());
    if flags.detach {
        let children = data
            .windows
            .iter()
            .find(|window| window.id == data.window)
            .into_iter()
            .flat_map(|window| &window.views)
            .map(|view| {
                let mut entry = item(&format!("win.detach.{}", view.id), &view.name, "");
                let kind = view.kind.clone();
                let accent = view.accent;
                entry.icon = crate::ItemIcon::paint(move |p, r, opacity| paint_view_glyph(p, r, &kind, accent.gamma_multiply(opacity)));
                entry
            })
            .collect();
        rows.push(crate::Item::submenu("layout.detach", &data.text.detach, crate::icons::window_icon(false), children));
    }
    rows.push(item("layout.reset_cameras", &data.text.reset, ""));
    let mut clean = item("layout.clean", &data.text.clean, "").key("F11");
    clean.checked = Some(data.clean);
    rows.push(clean);
    rows.push(crate::Item::sep());
    let saved_icon = crate::ItemIcon::paint(|p, r, opacity| paint_view_glyph(p, r, "tiles", Tokens::get().text_dim.gamma_multiply(opacity)));
    let mut save = item("layout.save_view", &data.text.save, "");
    save.icon = saved_icon.clone();
    rows.push(save);
    if !data.saved.is_empty() {
        let mut children = data
            .saved
            .iter()
            .map(|name| radio(&format!("layout.saved.{name}"), name, "", data.matching_saved.as_ref() == Some(name)))
            .collect::<Vec<_>>();
        children.push(crate::Item::sep());
        children.push(item("layout.manage", &data.text.manage, ""));
        let mut saved = crate::Item::submenu("layout.saved", &data.text.saved, "", children);
        saved.icon = saved_icon;
        rows.push(saved);
    }
    rows
}
