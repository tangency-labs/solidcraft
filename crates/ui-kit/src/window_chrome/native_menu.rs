//! The macOS menu bar, built from the kit's menu entries ([`Item`]) and answering with the id
//! (and parameters) of the entry chosen, for the app to run through its own command table.
//!
//! The bar follows the system's layout: the app menu (About, the app's entries, Services, Hide,
//! Hide Others, Show All, Quit), then the app's menus. The menu whose id is [`WINDOW`] also gets
//! the system's window entries (Minimize, Zoom, Enter Full Screen, Bring All to Front). An entry's
//! `shortcut` is a menu accelerator (`Cmd+Shift+S`, `Cmd+,`). A menu's own id (`menu.file`, ...)
//! names it for [`MenuTable::replace`] and is never dispatched, so an item with such an id is a
//! submenu even with no children yet. The layout ([`MenuTable`]) is the
//! same everywhere; only macOS turns it into native menus ([`NativeMenu`]); Windows and Linux keep
//! their default (no menu bar).

use crate::Item;
use serde_json::Value;

/// The id of the app's Window menu: the system's window entries are added to it.
pub const WINDOW: &str = "menu.window";
/// The id of the app menu (the one titled with the app's name).
pub const APP: &str = "menu.app";

/// What the app puts in its menu bar.
#[derive(Clone, Debug)]
pub struct MenuBar {
    /// The app's name: the app menu's title, About and Hide name it.
    pub title: String,
    /// Shown in About.
    pub version: String,
    /// The app menu's own entries, between About and Services (Settings, ...).
    pub app: Vec<Item>,
    /// The app menu's last entry.
    pub quit: Item,
    /// The menus after the app menu, left to right: each `Item`'s children are its entries.
    pub menus: Vec<Item>,
}

/// An entry the system provides and runs itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum System {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Minimize,
    Zoom,
    FullScreen,
    BringAllToFront,
}

/// One menu row. An entry carries the key its native item is made with; the [`MenuTable`] maps
/// it back to the entry's id and parameters.
#[derive(Debug)]
pub enum Row<'a> {
    Separator,
    /// A section heading: a row that can't be chosen.
    Heading(&'a str),
    Entry {
        key: String,
        item: &'a Item,
    },
    Submenu {
        item: &'a Item,
        rows: Vec<Row<'a>>,
    },
    System(System),
}

/// One menu of the bar, left to right.
#[derive(Debug)]
pub struct Menu<'a> {
    pub id: &'a str,
    pub label: &'a str,
    /// Off: the menu shows greyed and does not open.
    pub enabled: bool,
    pub rows: Vec<Row<'a>>,
}

#[derive(Debug)]
struct Entry {
    key: String,
    id: String,
    params: Option<Value>,
    /// The ids of the menus it sits in, outermost first.
    within: Vec<String>,
}

/// The bar's entries by the key of their native item, and its submenus.
#[derive(Debug, Default)]
pub struct MenuTable {
    entries: Vec<Entry>,
    /// Every submenu's id with the ids of the menus it sits in.
    submenus: Vec<(String, Vec<String>)>,
    next: u64,
}

impl MenuTable {
    /// Lays out the bar, registering every entry that can be chosen.
    pub fn layout<'a>(&mut self, bar: &'a MenuBar) -> Vec<Menu<'a>> {
        let app = [APP.to_string()];
        let mut rows = vec![Row::System(System::About), Row::Separator];
        rows.extend(self.rows(&bar.app, &app));
        rows.extend([Row::Separator, Row::System(System::Services), Row::Separator]);
        rows.extend([Row::System(System::Hide), Row::System(System::HideOthers), Row::System(System::ShowAll), Row::Separator]);
        rows.extend(self.rows(std::slice::from_ref(&bar.quit), &app));
        let mut menus = vec![Menu { id: APP, label: &bar.title, enabled: true, rows }];
        for menu in &bar.menus {
            let within = [menu.id.clone()];
            let mut rows = self.rows(&menu.children, &within);
            if menu.id == WINDOW {
                rows.extend([Row::Separator, Row::System(System::Minimize), Row::System(System::Zoom), Row::System(System::FullScreen)]);
                rows.extend([Row::Separator, Row::System(System::BringAllToFront)]);
            }
            menus.push(Menu { id: &menu.id, label: &menu.label, enabled: menu.enabled, rows });
        }
        menus
    }

    /// Replaces what the submenu with id `id` holds (a recent-files list): forgets its old
    /// entries and submenus and lays out `children` in their place. For submenus inside the
    /// bar's menus only: a menu of the bar itself (the app menu, File, Window, ...) keeps the
    /// system's entries the layout gave it, so it is built once and `replace` refuses it.
    pub fn replace<'a>(&mut self, id: &str, children: &'a [Item]) -> Result<Vec<Row<'a>>, String> {
        let Some((_, outer)) = self.submenus.iter().find(|(sid, _)| sid == id) else {
            return Err(format!("no submenu `{id}` in the menu bar"));
        };
        let mut within = outer.clone();
        within.push(id.to_string());
        let inside = |w: &[String]| w.iter().any(|m| m == id);
        self.entries.retain(|e| !inside(&e.within));
        self.submenus.retain(|(_, w)| !inside(w));
        Ok(self.rows(children, &within))
    }

    fn rows<'a>(&mut self, items: &'a [Item], within: &[String]) -> Vec<Row<'a>> {
        items
            .iter()
            .map(|item| {
                if item.is_sep() {
                    Row::Separator
                } else if item.is_heading() {
                    Row::Heading(&item.label)
                } else if !item.children.is_empty() || item.id.starts_with("menu.") {
                    self.submenus.push((item.id.clone(), within.to_vec()));
                    let mut inner = within.to_vec();
                    inner.push(item.id.clone());
                    Row::Submenu { item, rows: self.rows(&item.children, &inner) }
                } else {
                    self.next += 1;
                    let key = format!("menu-{}", self.next);
                    self.entries.push(Entry { key: key.clone(), id: item.id.clone(), params: item.params.clone(), within: within.to_vec() });
                    Row::Entry { key, item }
                }
            })
            .collect()
    }

    /// The id and parameters of the entry whose native item has `key`.
    pub fn find(&self, key: &str) -> Option<(&str, Option<&Value>)> {
        self.entries.iter().find(|e| e.key == key).map(|e| (e.id.as_str(), e.params.as_ref()))
    }

    /// Every entry: (key, id, parameters).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str, Option<&Value>)> {
        self.entries.iter().map(|e| (e.key.as_str(), e.id.as_str(), e.params.as_ref()))
    }

    /// The menus the submenu with id `id` sits in, outermost first; `None` for one the bar
    /// doesn't have.
    pub fn within(&self, id: &str) -> Option<&[String]> {
        self.submenus.iter().find(|(sid, _)| sid == id).map(|(_, w)| w.as_slice())
    }
}

#[cfg(target_os = "macos")]
pub use native::NativeMenu;

#[cfg(target_os = "macos")]
mod native {
    use super::{MenuBar, MenuTable, Row, System};
    use crate::Item;
    use muda::{AboutMetadata, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, accelerator::Accelerator};
    use serde_json::Value;
    use std::sync::mpsc::{Receiver, sync_channel};

    /// The app's menu bar while it lives; dropping it takes the bar down.
    pub struct NativeMenu {
        menu: Menu,
        rows: Rows,
        events: Receiver<MenuEvent>,
    }

    /// The native rows of a laid-out bar.
    struct Rows {
        table: MenuTable,
        title: String,
        version: String,
        items: Vec<(String, MenuItem)>,
        /// Every submenu by its id.
        submenus: Vec<(String, Submenu)>,
    }

    fn err(e: muda::Error) -> String {
        e.to_string()
    }

    impl NativeMenu {
        /// Builds `bar` and installs it as the app's menu bar. A chosen entry repaints `ctx`; read
        /// it with [`Self::events`]. On the main thread only, as every AppKit menu.
        pub fn new(bar: &MenuBar, ctx: &egui::Context) -> Result<Self, String> {
            // Rows first: the bar is only installed, and taken down on drop, once it is whole.
            let mut table = MenuTable::default();
            let menus = table.layout(bar);
            let mut rows = Rows { table, title: bar.title.clone(), version: bar.version.clone(), items: Vec::new(), submenus: Vec::new() };
            let menu = Menu::new();
            for m in menus {
                let sub = Submenu::new(m.label, m.enabled);
                rows.append(&sub, m.rows)?;
                menu.append(&sub).map_err(err)?;
            }
            let (sender, events) = sync_channel(64);
            let ctx = ctx.clone();
            MenuEvent::set_event_handler(Some(move |event| {
                let _ = sender.try_send(event);
                ctx.request_repaint();
            }));
            menu.init_for_nsapp();
            Ok(Self { menu, rows, events })
        }

        /// The entries chosen since the last call: (id, parameters).
        pub fn events(&mut self) -> Vec<(String, Option<Value>)> {
            let events: Vec<_> = self.events.try_iter().take(64).collect();
            events.iter().filter_map(|e| self.rows.table.find(e.id.as_ref())).map(|(id, params)| (id.to_string(), params.cloned())).collect()
        }

        /// Turns every entry on or off: `enabled(id, parameters)`.
        pub fn update(&self, enabled: impl Fn(&str, Option<&Value>) -> bool) {
            for (key, id, params) in self.rows.table.entries() {
                if let Some((_, item)) = self.rows.items.iter().find(|(k, _)| k == key) {
                    let on = enabled(id, params);
                    if item.is_enabled() != on {
                        item.set_enabled(on);
                    }
                }
            }
        }

        /// Replaces the entries of the submenu with id `id` (a recent-files list).
        pub fn replace(&mut self, id: &str, children: &[Item]) -> Result<(), String> {
            let sub = self
                .rows
                .submenus
                .iter()
                .find(|(sid, _)| sid == id)
                .map(|(_, s)| s.clone())
                .ok_or_else(|| format!("no submenu `{id}` in the menu bar"))?;
            let mut table = std::mem::take(&mut self.rows.table);
            let laid = table.replace(id, children);
            // What the table forgot goes from the native rows too.
            self.rows.items.retain(|(k, _)| table.find(k).is_some());
            self.rows.submenus.retain(|(sid, _)| sid == id || table.within(sid).is_some());
            self.rows.table = table;
            let laid = laid?;
            while sub.remove_at(0).is_some() {}
            self.rows.append(&sub, laid)
        }
    }

    impl Rows {
        fn system(&self, s: System) -> PredefinedMenuItem {
            let title = self.title.as_str();
            match s {
                System::About => PredefinedMenuItem::about(
                    Some(&format!("About {title}")),
                    Some(AboutMetadata { name: Some(title.into()), version: Some(self.version.clone()), ..Default::default() }),
                ),
                System::Services => PredefinedMenuItem::services(None),
                System::Hide => PredefinedMenuItem::hide(Some(&format!("Hide {title}"))),
                System::HideOthers => PredefinedMenuItem::hide_others(None),
                System::ShowAll => PredefinedMenuItem::show_all(None),
                System::Minimize => PredefinedMenuItem::minimize(None),
                System::Zoom => PredefinedMenuItem::maximize(None),
                System::FullScreen => PredefinedMenuItem::fullscreen(None),
                System::BringAllToFront => PredefinedMenuItem::bring_all_to_front(None),
            }
        }

        fn append(&mut self, parent: &Submenu, rows: Vec<Row<'_>>) -> Result<(), String> {
            for row in rows {
                let native: Box<dyn IsMenuItem> = match row {
                    Row::Separator => Box::new(PredefinedMenuItem::separator()),
                    Row::Heading(label) => Box::new(MenuItem::new(label, false, None)),
                    Row::System(s) => Box::new(self.system(s)),
                    Row::Entry { key, item } => {
                        let shortcut = match item.shortcut.trim() {
                            "" => None,
                            s => Some(s.parse::<Accelerator>().map_err(|e| format!("menu entry `{}`: shortcut `{s}`: {e}", item.id))?),
                        };
                        let native = MenuItem::with_id(key.as_str(), &item.label, item.enabled, shortcut);
                        self.items.push((key, native.clone()));
                        Box::new(native)
                    }
                    Row::Submenu { item, rows } => {
                        let sub = Submenu::new(&item.label, item.enabled);
                        self.append(&sub, rows)?;
                        self.submenus.push((item.id.clone(), sub.clone()));
                        Box::new(sub)
                    }
                };
                parent.append(native.as_ref()).map_err(err)?;
            }
            Ok(())
        }
    }

    impl Drop for NativeMenu {
        fn drop(&mut self) {
            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
            self.menu.remove_for_nsapp();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(id: &str, label: &str) -> Item {
        Item::action(id, label, "")
    }

    fn submenu(id: &str, label: &str, children: Vec<Item>) -> Item {
        let mut menu = entry(id, label);
        menu.children = children;
        menu
    }

    /// The rows as words: the entry's id, `-`, `#heading`, `[submenu: ...]` or the system entry.
    fn shape(rows: &[Row<'_>]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Separator => "-".to_string(),
                Row::Heading(h) => format!("#{h}"),
                Row::Entry { item, .. } => item.id.clone(),
                Row::Submenu { item, rows } => format!("[{}: {}]", item.id, shape(rows).join(" ")),
                Row::System(s) => format!("{s:?}"),
            })
            .collect()
    }

    fn keys<'a>(rows: &'a [Row<'a>], out: &mut Vec<&'a str>) {
        for r in rows {
            match r {
                Row::Entry { key, .. } => out.push(key),
                Row::Submenu { rows, .. } => keys(rows, out),
                _ => {}
            }
        }
    }

    fn bar() -> MenuBar {
        let recent = vec![
            entry("ui.file.open_recent", "/a.solidcraft").with(json!({"path": "/a.solidcraft"})),
            Item::sep(),
            entry("ui.file.clear_recent", "Clear"),
        ];
        MenuBar {
            title: "Craft".into(),
            version: "1.0".into(),
            app: vec![entry("ui.prefs", "Settings…").with(json!({"open": "General"})).key("Cmd+,")],
            quit: entry("app.quit", "Quit Craft").key("Cmd+Q"),
            menus: vec![
                submenu(
                    "menu.file",
                    "File",
                    vec![entry("file.new", "New").key("Cmd+N"), submenu("menu.file.recent", "Open Recent", recent), Item::sep()],
                ),
                submenu(WINDOW, "Window", vec![entry("ui.agent_chat", "Agent Chat")]).on_if(false),
            ],
        }
    }

    #[test]
    fn the_bar_follows_the_system_layout() {
        let bar = bar();
        let mut table = MenuTable::default();
        let menus = table.layout(&bar);
        let titles: Vec<_> = menus.iter().map(|m| (m.id, m.label, m.enabled)).collect();
        assert_eq!(titles, [(APP, "Craft", true), ("menu.file", "File", true), (WINDOW, "Window", false)], "a menu that is off stays off");
        assert_eq!(shape(&menus[0].rows), ["About", "-", "ui.prefs", "-", "Services", "-", "Hide", "HideOthers", "ShowAll", "-", "app.quit"]);
        assert_eq!(shape(&menus[1].rows), ["file.new", "[menu.file.recent: ui.file.open_recent - ui.file.clear_recent]", "-"]);
        assert_eq!(shape(&menus[2].rows), ["ui.agent_chat", "-", "Minimize", "Zoom", "FullScreen", "-", "BringAllToFront"]);
        // Menu ids are never entries.
        assert!(table.entries().all(|(_, id, _)| !id.starts_with("menu.")));
        assert_eq!(table.entries().count(), 6);
    }

    #[test]
    fn a_chosen_entry_maps_back_to_its_id_and_params_and_replace_forgets_the_old_ones() {
        let bar = bar();
        let mut table = MenuTable::default();
        let menus = table.layout(&bar);
        let mut file = vec![];
        keys(&menus[1].rows, &mut file);
        let file: Vec<String> = file.into_iter().map(str::to_string).collect();
        assert_eq!(table.find(&file[0]), Some(("file.new", None)));
        assert_eq!(table.find(&file[1]), Some(("ui.file.open_recent", Some(&json!({"path": "/a.solidcraft"})))));
        assert_eq!(table.find(&file[2]), Some(("ui.file.clear_recent", None)));
        assert_eq!(table.find("menu-999"), None);
        assert_eq!(table.within("menu.file.recent"), Some(&["menu.file".to_string()][..]));

        let fresh = vec![entry("ui.file.open_recent", "/b.solidcraft").with(json!({"path": "/b.solidcraft"}))];
        let rows = table.replace("menu.file.recent", &fresh).unwrap_or_default();
        let mut new = vec![];
        keys(&rows, &mut new);
        assert_eq!(new.len(), 1);
        assert_eq!(table.find(new[0]), Some(("ui.file.open_recent", Some(&json!({"path": "/b.solidcraft"})))));
        assert_eq!((table.find(&file[1]), table.find(&file[2])), (None, None), "the old recent entries are gone");
        assert_eq!(table.find(&file[0]), Some(("file.new", None)), "the rest of File stays");
        assert_eq!(table.entries().count(), 5);
        assert!(table.replace("menu.nothing", &fresh).is_err());
        for menu in [APP, "menu.file", WINDOW] {
            assert!(table.replace(menu, &fresh).is_err(), "{menu} is a menu of the bar, built once");
        }
        assert_eq!(table.entries().count(), 5, "a refused replace forgets nothing");
    }

    #[test]
    fn a_submenu_empty_at_build_time_stays_a_submenu_and_can_be_filled_later() {
        let mut bar = bar();
        bar.menus[0].children.push(submenu("menu.file.empty", "Nothing yet", vec![submenu("menu.file.empty.inner", "Nor here", vec![])]));
        let mut table = MenuTable::default();
        table.layout(&bar);
        for id in ["menu.file.empty", "menu.file.empty.inner"] {
            assert!(table.within(id).is_some(), "{id} is a submenu");
            assert!(table.entries().all(|(_, entry, _)| entry != id), "{id} is never dispatched");
        }
        let before = table.entries().count();

        let fresh = vec![entry("ui.file.open_recent", "/b.solidcraft")];
        let rows = table.replace("menu.file.empty", &fresh).unwrap_or_default();
        let mut new = vec![];
        keys(&rows, &mut new);
        assert_eq!(new.len(), 1);
        assert_eq!(table.find(new[0]), Some(("ui.file.open_recent", None)));
        assert_eq!(table.entries().count(), before + 1);
    }
}
