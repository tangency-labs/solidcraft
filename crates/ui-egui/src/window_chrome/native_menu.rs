//! The macOS menu bar, built from the kit's menu entries ([`Item`]) and answering with the id
//! (and parameters) of the entry chosen, for the app to run through its own command table.
//!
//! The bar follows the system's layout: the app menu (About, the app's entries, Services, Hide,
//! Hide Others, Show All, Quit), then the app's menus. The menu whose id is [`WINDOW`] also gets
//! the system's window entries (Minimize, Zoom, Enter Full Screen, Bring All to Front). An entry's
//! `shortcut` is a menu accelerator (`Cmd+Shift+S`, `Cmd+,`). Windows and Linux keep their default
//! (no menu bar), so only the entry table builds there.

use crate::ui_kit::Item;
use serde_json::Value;

/// The id of the app's Window menu: the system's window entries are added to it.
pub const WINDOW: &str = "ui.window";

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

/// One menu row, as laid out from an [`Item`] tree. Entries carry the key their native item
/// is made with; the [`MenuTable`] maps it back to the entry's id and parameters.
#[derive(Debug)]
pub(crate) enum Node<'a> {
    Separator,
    /// A section heading: a row that can't be chosen.
    Heading(&'a str),
    Entry {
        key: String,
        item: &'a Item,
    },
    Submenu {
        item: &'a Item,
        children: Vec<Node<'a>>,
    },
}

/// The menu's entries by the key of their native item.
#[derive(Debug, Default)]
pub struct MenuTable {
    entries: Vec<(String, String, Option<Value>)>,
    next: u64,
}

impl MenuTable {
    /// Lays out `items`, registering every entry that can be chosen.
    pub(crate) fn layout<'a>(&mut self, items: &'a [Item]) -> Vec<Node<'a>> {
        items
            .iter()
            .map(|item| {
                if item.is_sep() {
                    Node::Separator
                } else if item.is_heading() {
                    Node::Heading(&item.label)
                } else if !item.children.is_empty() {
                    Node::Submenu { item, children: self.layout(&item.children) }
                } else {
                    self.next += 1;
                    let key = format!("menu-{}", self.next);
                    self.entries.push((key.clone(), item.id.clone(), item.params.clone()));
                    Node::Entry { key, item }
                }
            })
            .collect()
    }

    /// The id and parameters of the entry whose native item has `key`.
    pub fn find(&self, key: &str) -> Option<(&str, Option<&Value>)> {
        self.entries.iter().find(|(k, ..)| k == key).map(|(_, id, params)| (id.as_str(), params.as_ref()))
    }

    /// Every entry: (key, id, parameters).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str, Option<&Value>)> {
        self.entries.iter().map(|(k, id, params)| (k.as_str(), id.as_str(), params.as_ref()))
    }

    fn forget(&mut self, keys: &[String]) {
        self.entries.retain(|(k, ..)| !keys.contains(k));
    }
}

#[cfg(target_os = "macos")]
pub use native::NativeMenu;

#[cfg(target_os = "macos")]
mod native {
    use super::{MenuBar, MenuTable, Node, WINDOW};
    use crate::ui_kit::Item;
    use muda::{AboutMetadata, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, accelerator::Accelerator};
    use serde_json::Value;
    use std::sync::mpsc::{Receiver, sync_channel};

    /// The app's menu bar while it lives; dropping it takes the bar down.
    pub struct NativeMenu {
        menu: Menu,
        rows: Rows,
        events: Receiver<MenuEvent>,
    }

    /// The native rows and what they stand for.
    #[derive(Default)]
    struct Rows {
        table: MenuTable,
        items: Vec<(String, MenuItem)>,
        /// Every submenu by its id, with the keys and submenu ids laid out under it.
        submenus: Vec<(String, Submenu, Vec<String>, Vec<String>)>,
    }

    fn err(e: muda::Error) -> String {
        e.to_string()
    }

    impl NativeMenu {
        /// Builds `bar` and installs it as the app's menu bar. A chosen entry repaints `ctx`; read
        /// it with [`Self::events`]. On the main thread only, as every AppKit menu.
        pub fn new(bar: &MenuBar, ctx: &egui::Context) -> Result<Self, String> {
            // Rows first: the bar is only installed, and taken down on drop, once it is whole.
            let (menu, mut rows) = (Menu::new(), Rows::default());
            let title = bar.title.as_str();
            let application = Submenu::new(title, true);
            application
                .append(&PredefinedMenuItem::about(
                    Some(&format!("About {title}")),
                    Some(AboutMetadata { name: Some(title.into()), version: Some(bar.version.clone()), ..Default::default() }),
                ))
                .map_err(err)?;
            application.append(&PredefinedMenuItem::separator()).map_err(err)?;
            rows.fill(&application, &bar.app)?;
            application.append(&PredefinedMenuItem::separator()).map_err(err)?;
            application.append(&PredefinedMenuItem::services(None)).map_err(err)?;
            application.append(&PredefinedMenuItem::separator()).map_err(err)?;
            application
                .append_items(&[
                    &PredefinedMenuItem::hide(Some(&format!("Hide {title}"))),
                    &PredefinedMenuItem::hide_others(None),
                    &PredefinedMenuItem::show_all(None),
                    &PredefinedMenuItem::separator(),
                ])
                .map_err(err)?;
            rows.fill(&application, std::slice::from_ref(&bar.quit))?;
            menu.append(&application).map_err(err)?;
            for item in &bar.menus {
                let nodes = rows.table.layout(&item.children);
                let sub = rows.submenu(item, nodes)?;
                if item.id == WINDOW {
                    sub.append_items(&[
                        &PredefinedMenuItem::separator(),
                        &PredefinedMenuItem::minimize(None),
                        &PredefinedMenuItem::maximize(None),
                        &PredefinedMenuItem::fullscreen(None),
                        &PredefinedMenuItem::separator(),
                        &PredefinedMenuItem::bring_all_to_front(None),
                    ])
                    .map_err(err)?;
                }
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
            let Some((_, sub, keys, subs)) = self.rows.submenus.iter_mut().find(|(sid, ..)| sid == id) else {
                return Err(format!("no submenu `{id}` in the menu bar"));
            };
            let (sub, keys, subs) = (sub.clone(), std::mem::take(keys), std::mem::take(subs));
            while sub.remove_at(0).is_some() {}
            self.rows.table.forget(&keys);
            self.rows.items.retain(|(k, _)| !keys.contains(k));
            self.rows.submenus.retain(|(sid, ..)| !subs.contains(sid));
            let (keys, subs) = self.rows.fill(&sub, children)?;
            if let Some((_, _, k, s)) = self.rows.submenus.iter_mut().find(|(sid, ..)| sid == id) {
                (*k, *s) = (keys, subs);
            }
            Ok(())
        }
    }

    impl Rows {
        fn submenu(&mut self, item: &Item, children: Vec<Node<'_>>) -> Result<Submenu, String> {
            let sub = Submenu::new(&item.label, item.enabled);
            let (keys, subs) = self.append(&sub, children)?;
            self.submenus.push((item.id.clone(), sub.clone(), keys, subs));
            Ok(sub)
        }

        /// Appends `items` to `parent`: the keys and submenu ids laid out under it.
        fn fill(&mut self, parent: &Submenu, items: &[Item]) -> Result<(Vec<String>, Vec<String>), String> {
            let nodes = self.table.layout(items);
            self.append(parent, nodes)
        }

        fn append(&mut self, parent: &Submenu, nodes: Vec<Node<'_>>) -> Result<(Vec<String>, Vec<String>), String> {
            let (mut keys, mut subs) = (Vec::new(), Vec::new());
            for node in nodes {
                let row: Box<dyn IsMenuItem> = match node {
                    Node::Separator => Box::new(PredefinedMenuItem::separator()),
                    Node::Heading(label) => Box::new(MenuItem::new(label, false, None)),
                    Node::Entry { key, item } => {
                        let shortcut = match item.shortcut.trim() {
                            "" => None,
                            s => Some(s.parse::<Accelerator>().map_err(|e| format!("menu entry `{}`: shortcut `{s}`: {e}", item.id))?),
                        };
                        let row = MenuItem::with_id(key.as_str(), &item.label, item.enabled, shortcut);
                        keys.push(key.clone());
                        self.items.push((key, row.clone()));
                        Box::new(row)
                    }
                    Node::Submenu { item, children } => {
                        let sub = self.submenu(item, children)?;
                        // `submenu` registered it last, with what it holds.
                        if let Some((_, _, k, s)) = self.submenus.last() {
                            keys.extend(k.iter().cloned());
                            subs.extend(s.iter().cloned());
                        }
                        subs.push(item.id.clone());
                        Box::new(sub)
                    }
                };
                parent.append(row.as_ref()).map_err(err)?;
            }
            Ok((keys, subs))
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

    #[test]
    fn a_chosen_entry_maps_back_to_its_id_and_params() {
        let mut recent = entry("ui.file.recent_menu", "Open Recent");
        recent.children = vec![entry("ui.file.open_recent", "/a.solidcraft").with(json!({"path": "/a.solidcraft"})), Item::sep()];
        let mut file = entry("ui.file", "File");
        file.children = vec![entry("ui.file.new", "New").key("Cmd+N"), recent, Item::sep(), entry("file.insert_step", "Insert STEP…")];
        let items = vec![file];
        let mut table = MenuTable::default();
        let nodes = table.layout(&items);
        let [Node::Submenu { children, .. }] = nodes.as_slice() else { panic!("one submenu: {nodes:?}") };
        let keys: Vec<&str> = children
            .iter()
            .flat_map(|n| match n {
                Node::Entry { key, .. } => vec![key.as_str()],
                Node::Submenu { children, .. } => {
                    children.iter().filter_map(|n| if let Node::Entry { key, .. } = n { Some(key.as_str()) } else { None }).collect()
                }
                _ => vec![],
            })
            .collect();
        assert_eq!(keys.len(), 3, "separators and submenus are not entries: {nodes:?}");
        assert_eq!(table.find(keys[0]), Some(("ui.file.new", None)));
        assert_eq!(table.find(keys[1]), Some(("ui.file.open_recent", Some(&json!({"path": "/a.solidcraft"})))));
        assert_eq!(table.find(keys[2]), Some(("file.insert_step", None)));
        assert_eq!(table.find("menu-999"), None);
        assert_eq!(table.entries().count(), 3);
        table.forget(&[keys[1].to_string()]);
        assert_eq!(table.find(keys[1]), None);
    }
}
