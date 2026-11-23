//! Manifest placements drawn by the host, opening the Plugins menu entry dialog.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::{SolveApp, plugins, ui_kit};

/// How often the folder watcher looks at the plugins' directory ([`start_watch`]): a plugin
/// dropped in while the app runs appears within about this long, and an idle app draws nothing
/// meanwhile.
#[cfg(not(target_arch = "wasm32"))]
const WATCH_MS: u64 = 2000;
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_engine::plugin::UiSlot;

#[derive(Clone)]
pub(crate) struct Item {
    pub command: String,
    pub plugin: String,
    pub entry: String,
    pub label: String,
    pub slot: Slot,
    /// An importer chooses its own file, so it needs no open design.
    pub importer: bool,
    /// The command's icon: its own from the package, else the generic plugin icon.
    pub icon: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Menu(&'static str),
    Ribbon(&'static str),
}
#[derive(Default)]
pub(crate) struct State {
    items: Vec<Item>,
    /// When the plugins' folder was last read, and which folder: another root is read afresh,
    /// so a design or a test that points the app at its own folder does not wait for a change.
    scanned: Option<(f64, Option<PathBuf>)>,
    discovery: Option<Receiver<Inventory>>,
    inventory: Option<Inventory>,
    /// The window, set by the app bar each frame: a finished scan and the folder watcher ask for
    /// the frame that takes their news through it, so nothing polls while the folder is settled.
    #[cfg(not(target_arch = "wasm32"))]
    ctx: Option<egui::Context>,
    #[cfg(not(target_arch = "wasm32"))]
    watch: Option<Watch>,
}

/// A thread that looks at the plugins' directory every [`WATCH_MS`], and asks for a frame only
/// when what it holds changed: the alternative to rescanning the folder on a timer.
#[cfg(not(target_arch = "wasm32"))]
struct Watch {
    root: Option<PathBuf>,
    changed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Watch {
    fn drop(&mut self) {
        // The thread checks after each sleep; joining would hold the window for up to WATCH_MS.
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Inventory {
    root: Option<PathBuf>,
    plugins: Vec<plugins::Plugin>,
    skipped: Vec<plugins::Skipped>,
}

/// Preferences reuses discovery's package snapshot instead of hashing components every frame.
pub(crate) fn preferences_inventory(app: &mut SolveApp, dir: &Path) -> Vec<plugins::Plugin> {
    if app.plugin_slots.inventory.as_ref().is_none_or(|inventory| inventory.root.as_deref() != Some(dir)) {
        let (plugins, skipped) = plugins::inventory(Some(dir), app.session.plugin_state.packages.values());
        app.plugin_slots.inventory = Some(Inventory { root: Some(dir.to_path_buf()), plugins, skipped });
    }
    app.plugin_slots.inventory.as_ref().map(|inventory| inventory.plugins.clone()).unwrap_or_default()
}

pub(crate) fn skipped(app: &SolveApp) -> &[plugins::Skipped] {
    app.plugin_slots.inventory.as_ref().map(|inventory| inventory.skipped.as_slice()).unwrap_or_default()
}
/// The window the app bar draws into: a finished scan and the folder watcher ask for the frame
/// that takes their news through it, so nothing polls while the folder is settled.
pub(crate) fn set_context(app: &mut SolveApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.plugin_slots.ctx = Some(ctx.clone());
    }
    // The web has no threads: nothing to wake, and the plugins are read when the page loads.
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}
/// The ribbon tab a manifest's `ribbon` slot names, when it is one besides Model. Model's Plugins
/// group already holds every plugin command (and labels it with the slot's name), so a slot that
/// names Model or no tab is not drawn a second time.
pub(crate) fn ribbon_name(name: &str) -> Option<&'static str> {
    crate::workspace::choosable_tabs().chain(["SKETCH"]).find(|tab| tab.eq_ignore_ascii_case(name.trim())).filter(|tab| *tab != "MODEL")
}
pub(crate) fn menu_name(name: &str) -> Option<&'static str> {
    match name.trim().to_ascii_lowercase().as_str() {
        "file" => Some("File"),
        "help" => Some("Help"),
        _ => None,
    }
}
#[cfg(not(target_arch = "wasm32"))]
fn declared(plugin: &plugins::Plugin) -> Vec<Item> {
    let mut items = Vec::new();
    for entry in &plugin.entries {
        for slot in &entry.slots {
            let slot = match slot {
                UiSlot::Menu { name } => {
                    let Some(name) = menu_name(name) else { continue };
                    Slot::Menu(name)
                }
                UiSlot::Ribbon { name } => {
                    let Some(name) = ribbon_name(name) else { continue };
                    Slot::Ribbon(name)
                }
            };
            let command = format!("{}.{}", plugin.id, entry.name);
            if solidcraft_engine::find_command(&command).is_some() {
                continue;
            }
            if items.iter().any(|item: &Item| item.command == command && item.slot == slot) {
                continue;
            }
            items.push(Item {
                command,
                plugin: plugin.name.clone(),
                entry: entry.name.clone(),
                label: format!("{} · {}", plugin.name, entry.name),
                slot,
                importer: entry.kind == solidcraft_engine::plugin::EntryKind::Importer,
                icon: crate::plugin_menu::command_icon(plugin, entry).to_owned(),
            });
        }
    }
    items
}
pub(crate) fn invalidate(app: &mut SolveApp) {
    app.plugin_slots.discovery = None;
    app.plugin_slots.items.clear();
    app.plugin_slots.scanned = None;
    app.plugin_slots.inventory = None;
    // The Plugins menu and the Model tab's Plugins group read the installed plugins again.
    app.plugin_menu.loaded = false;
}

fn discovered(app: &mut SolveApp, inventory: Inventory) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.plugin_slots.items = inventory.plugins.iter().flat_map(declared).take(8192).collect();
    }
    let root = inventory.root.clone();
    app.plugin_slots.inventory = Some(inventory);
    app.plugin_slots.scanned = Some((crate::now_ms(), root));
}
pub(crate) fn refresh(app: &mut SolveApp) {
    #[cfg(target_arch = "wasm32")]
    let _ = app;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let root = app.session.plugin_state.plugins_dir();
        start_watch(app, root.clone(), std::time::Duration::from_millis(WATCH_MS));
        // A finished scan lands here: the worker asked for this frame once, when it had the result.
        if let Some(rx) = &app.plugin_slots.discovery {
            match rx.try_recv() {
                Ok(inventory) => {
                    discovered(app, inventory);
                    app.plugin_slots.discovery = None;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    app.plugin_slots.discovery = None;
                }
            }
        }
        // This folder was read, and nothing in it changed since: there is nothing to do, and no
        // frame is asked for (the folder watcher wakes the window when it changes). A different
        // folder is read afresh.
        let scanned_here = app.plugin_slots.scanned.as_ref().is_some_and(|(_, at)| *at == root);
        let changed = app.plugin_slots.watch.as_ref().is_some_and(|w| w.changed.swap(false, Ordering::Relaxed));
        if scanned_here && !changed {
            return;
        }
        start_scan(app, root);
    }
}

/// Read the plugins' directory once, on a worker: when it finishes it sends the inventory and
/// asks for one frame, so the app bar needs no timer while the scan runs.
#[cfg(not(target_arch = "wasm32"))]
fn start_scan(app: &mut SolveApp, root: Option<PathBuf>) {
    let inspected = app.session.plugin_state.packages.clone();
    let ctx = app.plugin_slots.ctx.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("plugin-discovery".into()).spawn(move || {
        let (plugins, skipped) = plugins::inventory(root.as_deref(), inspected.values());
        let _ = tx.send(Inventory { root, plugins, skipped });
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    });
    match spawned {
        Ok(_) => app.plugin_slots.discovery = Some(rx),
        // No thread (very unlikely): retry on a later frame rather than leave the menu empty.
        Err(_) => app.plugin_slots.scanned = None,
    }
}

/// Watch the plugins' directory for changes, once per root: a plugin dropped in while the app
/// runs asks for a frame, which rescans the folder. Nothing is read while it stays the same.
#[cfg(not(target_arch = "wasm32"))]
fn start_watch(app: &mut SolveApp, root: Option<PathBuf>, every: std::time::Duration) {
    let Some(ctx) = app.plugin_slots.ctx.clone() else { return };
    if app.plugin_slots.watch.as_ref().is_some_and(|w| w.root == root) {
        return;
    }
    let changed = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));
    let (flag, halt, watched) = (changed.clone(), stop.clone(), root.clone());
    let spawned = std::thread::Builder::new().name("plugin-folder".into()).spawn(move || {
        let mut seen = signature(watched.as_deref());
        while !halt.load(Ordering::Relaxed) {
            std::thread::sleep(every);
            if halt.load(Ordering::Relaxed) {
                return;
            }
            let now = signature(watched.as_deref());
            if now != seen {
                seen = now;
                flag.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        }
    });
    // A watcher that cannot start is harmless: the folder is read when the design opens and when
    // Preferences or the Plugins menu ask, and a restart finds a plugin dropped in meanwhile.
    if spawned.is_ok() {
        app.plugin_slots.watch = Some(Watch { root, changed, stop });
    }
}

/// What the plugins' directory holds, cheaply: whether it exists, and each entry's name, size and
/// modification time. A dropped-in, replaced or removed plugin changes it; nothing else does.
#[cfg(not(target_arch = "wasm32"))]
fn signature(dir: Option<&Path>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut items: Vec<(String, u64, u128)> = Vec::new();
    let present = dir.is_some_and(Path::exists);
    if let Some(entries) = dir.and_then(|dir| std::fs::read_dir(dir).ok()) {
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            items.push((entry.file_name().to_string_lossy().into_owned(), meta.len(), modified));
        }
    }
    items.sort();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    present.hash(&mut h);
    items.hash(&mut h);
    h.finish()
}
pub(crate) fn items(app: &SolveApp, slot: Slot) -> Vec<Item> {
    app.plugin_slots
        .items
        .iter()
        .filter(|item| item.slot == slot && item.command.split_once('.').is_some_and(|(id, _)| plugins::enabled(app, id)))
        .cloned()
        .collect()
}

/// Open the same input-schema dialog as the Plugins menu, resolving the namespaced entry id.
pub(crate) fn begin(app: &mut SolveApp, command: &str) {
    if solidcraft_engine::find_command(command).is_some() {
        return;
    }
    app.plugin_menu.listed = crate::plugin_menu::list(app, app.session.plugin_state.plugins_dir().as_deref());
    let entry = app.plugin_menu.listed.iter().enumerate().find_map(|(p, plugin)| {
        if plugin.disabled {
            return None;
        }
        plugin.entries.iter().position(|entry| format!("{}.{}", plugin.id, entry.name) == command).map(|e| (p, e))
    });
    if let Some((p, e)) = entry {
        crate::plugin_menu::open_entry(app, p, e);
    }
}

pub(crate) fn menu_rows(app: &mut SolveApp, ui: &mut egui::Ui, name: &'static str) -> bool {
    let entries = items(app, Slot::Menu(name));
    if entries.is_empty() {
        return false;
    }
    ui.separator();
    for item in entries {
        let row = ui_kit::Item::action(&item.command, &item.label, &item.icon);
        if ui_kit::menu::list_row(ui, &row).is_some() {
            begin(app, &item.command);
            return true;
        }
    }
    false
}

pub(crate) struct RibbonGroup {
    key: String,
    name: String,
    entries: Vec<Item>,
    popup: egui::Id,
    open: bool,
}
pub(crate) fn ribbon_groups(app: &SolveApp, ui: &egui::Ui, tab: &str) -> Vec<RibbonGroup> {
    let mut groups: Vec<RibbonGroup> = Vec::new();
    let Some(slot) = ribbon_name(tab) else { return groups };
    for item in items(app, Slot::Ribbon(slot)) {
        let Some((id, _)) = item.command.split_once('.') else { continue };
        let key = format!("{tab}:plugin:{id}");
        if let Some(group) = groups.iter_mut().find(|g| g.key == key) {
            group.entries.push(item);
        } else {
            let popup = ui.scope_id().with(("plugin_ribbon", &key));
            let open = ui.ctx().data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open"))) == Some(popup);
            groups.push(RibbonGroup { key, name: item.plugin.clone(), entries: vec![item], popup, open });
        }
    }
    groups
}
impl RibbonGroup {
    pub(crate) fn group(&self, ui: &egui::Ui) -> crate::ribbon::Group<'_> {
        use crate::ribbon;
        ribbon::Group {
            key: &self.key,
            name: &self.name,
            label: &self.name,
            tip: &self.name,
            buttons: self
                .entries
                .iter()
                .map(|entry| ribbon::Button {
                    id: &entry.command,
                    run: &entry.command,
                    label: &entry.entry,
                    name: &entry.label,
                    icon: "prefs_plugins",
                    size: ribbon::ButtonSize { width: ribbon::button_width(ui.painter(), &entry.entry), min_width: ribbon::BUTTON_MIN_WIDTH },
                    planned: false,
                    enabled: true,
                    pressed: false,
                    exit: false,
                    split: None,
                    app_paints: false,
                    covers: &[],
                })
                .collect(),
            cells: vec![],
            commands: self.entries.iter().map(|e| e.command.as_str()).collect(),
            icon: Some("prefs_plugins"),
            enabled: true,
            open: self.open,
            running: None,
            more: self
                .entries
                .iter()
                .map(|e| ribbon::MoreRow {
                    id: &e.command,
                    label: &e.label,
                    icon: "prefs_plugins",
                    shortcut: "",
                    enabled: true,
                    checked: None,
                    hint: None,
                })
                .collect(),
        }
    }
    pub(crate) fn toggle(&self, ctx: &egui::Context, toggle: bool) {
        ctx.data_mut(|d| {
            let key = egui::Id::unique("sc_panel_open");
            if toggle && d.get_temp::<egui::Id>(key) == Some(self.popup) {
                d.remove::<egui::Id>(key);
            } else {
                d.insert_temp(key, self.popup);
            }
        });
    }
    pub(crate) fn popup(&self, app: &mut SolveApp, ui: &egui::Ui, drawn: &crate::ribbon::GroupOut) -> Option<(egui::Id, egui::Rect)> {
        let ctx = ui.ctx();
        if ctx.data(|d| d.get_temp::<egui::Id>(egui::Id::unique("sc_panel_open"))) != Some(self.popup) {
            return None;
        }
        let mut chosen = None;
        let out = ui_kit::Container::new(ctx, ui_kit::Surface::Popup)
            .area(self.popup)
            .fixed_pos(drawn.label.left_bottom())
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui_kit::Menu::show_frame(ui, ui_kit::Menu::frame(), |ui| {
                    ui_kit::Menu::heading(ui, &self.name);
                    for item in &self.entries {
                        if ui_kit::menu::list_row(ui, &ui_kit::Item::action(&item.command, &item.label, &item.icon)).is_some() {
                            chosen = Some(item.command.clone());
                        }
                    }
                });
            });
        if let Some(command) = chosen {
            begin(app, &command);
            ctx.data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
        } else if ctx.input(|i| i.key_pressed(egui::Key::Escape))
            || (ctx.input(|i| i.pointer.any_pressed()) && !out.response.contains_pointer() && !drawn.label_response.contains_pointer())
        {
            ctx.data_mut(|d| d.remove::<egui::Id>(egui::Id::unique("sc_panel_open")));
        }
        Some((self.popup, out.response.rect))
    }
}

/// The finish group remains last; the app still addresses its original built-in group indices.
pub(crate) fn restore_group_indices(out: &mut crate::ribbon::Out, builtin: usize, plugins: usize, exit: bool) {
    if !exit || builtin == 0 || plugins == 0 {
        return;
    }
    let original = |group: usize| {
        if group == builtin + plugins - 1 {
            builtin - 1
        } else if group >= builtin - 1 {
            group + 1
        } else {
            group
        }
    };
    for button in &mut out.buttons {
        button.group = original(button.group);
    }
    for group in &mut out.groups {
        group.group = original(group.group);
    }
    for action in &mut out.actions {
        match action {
            crate::ribbon::Action::Section { group } | crate::ribbon::Action::Open { group } | crate::ribbon::Action::Drop { group, .. } => {
                *group = original(*group)
            }
            _ => {}
        }
    }
}

#[cfg(test)]
pub(crate) fn wait_for_discovery(app: &mut SolveApp) {
    if let Some(rx) = app.plugin_slots.discovery.take() {
        discovered(app, rx.recv().unwrap());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plugin_unknown_menus_and_ribbons_have_no_second_placement_beside_the_model_plugins_group() {
        let plugin = plugins::Plugin {
            id: "helper".into(),
            name: "Helper".into(),
            version: "1".into(),
            description: String::new(),
            path: String::new(),
            approval: String::new(),
            disabled: false,
            groups: Default::default(),
            images: Default::default(),
            swatches: Default::default(),
            entries: serde_json::from_value(json!([
                {"name":"report","effect":"read","slots":[
                    {"kind":"menu","name":"Manufacturing"}, {"kind":"menu","name":"Plugins"},
                    {"kind":"ribbon","name":"Manufacturing"}, {"kind":"ribbon","name":"Model"}, {"kind":"ribbon","name":"inspect"}
                ]},
                {"name":"measure","effect":"read","slots":[
                    {"kind":"menu","name":" file "}, {"kind":"menu","name":"FILE"}, {"kind":"menu","name":"hElP"}
                ]}
            ]))
            .unwrap(),
        };
        let items = declared(&plugin);
        assert_eq!(items.len(), 3, "unmapped menus and the Model tab are omitted and repeated placements are deduplicated");
        assert_eq!(items[0].slot, Slot::Ribbon("INSPECT"));
        assert_eq!(items[1].slot, Slot::Menu("File"));
        assert_eq!(items[2].slot, Slot::Menu("Help"));
        assert_eq!(menu_name("Manufacturing"), None);
    }

    #[test]
    fn plugin_slots_share_inspected_packages_and_dialog_grant_policy_with_the_plugins_menu() {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("inspected/helper");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("plugin.wasm"), []).unwrap();
        std::fs::write(dir.join("plugin.json"), json!({
            "id":"helper", "name":"Helper", "version":"1", "capabilities":["commands@0.1"],
            "runtime":{"module":"plugin.wasm","commands":[],"permissions":{"command_groups":["file"]},
                "entries":[{"name":"report","effect":"read","input_schema":{"type":"object","properties":{"count":{"type":"integer","default":3}}},
                    "slots":[{"kind":"menu","name":"Help"}]}]}
        }).to_string()).unwrap();
        h.app.session.plugin_state.plugins_dir = Some(h.dir.join("installed"));
        h.app.session.plugin_state.packages.insert("helper".into(), dir.to_string_lossy().into_owned());
        invalidate(&mut h.app);
        refresh(&mut h.app);
        wait_for_discovery(&mut h.app);
        assert_eq!(items(&h.app, Slot::Menu("Help"))[0].command, "helper.report");
        let preferences = preferences_inventory(&mut h.app, &h.dir.join("installed"));
        assert_eq!(preferences.len(), 1, "Preferences includes the same inspected package as the slots and Plugins menu");
        assert_eq!(preferences[0].entries[0].name, "report");
        assert_eq!(preferences[0].path, dir.to_string_lossy());
        begin(&mut h.app, "helper.report");
        let slot = h.app.plugin_menu.dialogs.pop().unwrap();
        assert!(slot.ready(&h.app).unwrap_err().contains("file"));
        assert!(h.app.control_held.is_empty(), "missing grants never queue a run");
        crate::plugin_menu::open_entry(&mut h.app, 0, 0);
        let menu = h.app.plugin_menu.dialog().unwrap();
        assert_eq!(slot.fields, menu.fields);
        assert_eq!(slot.plugin.approval, menu.plugin.approval);
        assert_eq!(slot.ready(&h.app), menu.ready(&h.app));
        h.app.session.plugin_state.grants.entry("helper".into()).or_default().command_groups.insert("file".into());
        assert_eq!(menu.ready(&h.app), Ok(json!({"count":3})));
        h.app.preferences.disabled_plugins.insert("helper".into());
        assert!(menu.ready(&h.app).is_err(), "disabling an already open dialog prevents running it");
        h.app.plugin_menu.dialogs.clear();
        assert!(items(&h.app, Slot::Menu("Help")).is_empty());
        begin(&mut h.app, "helper.report");
        assert!(h.app.plugin_menu.dialogs.is_empty());
        h.app.preferences.disabled_plugins.clear();
        for command in ["helper.missing", "missing.report", "solid.box", "helper.report.extra"] {
            begin(&mut h.app, command);
            assert!(h.app.plugin_menu.dialogs.is_empty(), "{command}");
        }
        std::fs::remove_file(dir.join("plugin.wasm")).unwrap();
        begin(&mut h.app, "helper.report");
        assert!(h.app.plugin_menu.dialogs.is_empty(), "a package the shared reader refuses cannot open a slot dialog");
    }

    /// The plugins folder's signature: what the folder watcher compares between looks. It holds
    /// no clock of its own, so a folder that did not change reads the same twice.
    #[test]
    fn the_folder_signature_tracks_what_it_holds() {
        let dir = std::env::temp_dir().join(format!("sc-plugin-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(signature(Some(&dir)), signature(Some(&dir)), "a missing folder is stable");
        std::fs::create_dir_all(&dir).unwrap();
        let empty = signature(Some(&dir));
        assert_ne!(empty, signature(Some(&dir.join("absent"))), "a folder appearing is a change");
        assert_eq!(empty, signature(Some(&dir)), "an empty folder is stable");
        std::fs::write(dir.join("helper.json"), b"{}").unwrap();
        assert_ne!(empty, signature(Some(&dir)), "a dropped-in plugin is a change");
        let with_file = signature(Some(&dir));
        std::fs::write(dir.join("helper.json"), b"{\"id\":\"helper\"}").unwrap();
        assert_ne!(with_file, signature(Some(&dir)), "a replaced plugin is a change");
        std::fs::remove_file(dir.join("helper.json")).unwrap();
        assert_eq!(empty, signature(Some(&dir)), "removing it is a change back");
        assert_eq!(signature(None), signature(None), "no folder at all is stable");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A frame with the folder settled starts no scan and asks for no frame; the scan that does
    /// run asks for exactly one. The app bar no longer polls while discovery has nothing to do.
    #[test]
    fn a_settled_folder_asks_for_no_frame_and_a_finished_scan_asks_for_one() {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("installed");
        std::fs::create_dir_all(&dir).unwrap();
        h.app.session.plugin_state.plugins_dir = Some(dir);
        let ctx = egui::Context::default();
        set_context(&mut h.app, &ctx);
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = asked.clone();
        ctx.set_repaint_observer(move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        invalidate(&mut h.app);
        refresh(&mut h.app);
        wait_for_discovery(&mut h.app);
        wait_until(|| asked.load(Ordering::Relaxed) >= 1);
        assert_eq!(asked.load(Ordering::Relaxed), 1, "a finished scan asks for exactly one frame");
        refresh(&mut h.app);
        assert!(h.app.plugin_slots.discovery.is_none(), "a settled folder starts no scan");
        assert_eq!(asked.load(Ordering::Relaxed), 1, "and asks for no frame");
    }

    /// A plugin dropped into the folder wakes the window once, and the next frame reads it: the
    /// watcher replaces rescanning the folder on a timer, so a quiet folder costs nothing.
    #[test]
    fn a_plugin_dropped_into_the_folder_wakes_the_window() {
        let mut h = crate::scenario::Harness::new();
        let dir = h.dir.join("installed");
        std::fs::create_dir_all(&dir).unwrap();
        h.app.session.plugin_state.plugins_dir = Some(dir.clone());
        let ctx = egui::Context::default();
        set_context(&mut h.app, &ctx);
        let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = asked.clone();
        ctx.set_repaint_observer(move |_| {
            count.fetch_add(1, Ordering::Relaxed);
        });
        // A short look interval, so a dropped-in plugin is seen within the test's patience:
        // the frame's own `refresh` keeps this watcher (its root is the same).
        start_watch(&mut h.app, Some(dir.clone()), std::time::Duration::from_millis(10));
        invalidate(&mut h.app);
        refresh(&mut h.app);
        wait_for_discovery(&mut h.app);
        assert_eq!(asked.load(Ordering::Relaxed), 1, "the first scan asked for its frame");
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::create_dir_all(dir.join("helper")).unwrap();
        wait_until(|| asked.load(Ordering::Relaxed) >= 2);
        assert!(h.app.plugin_slots.watch.as_ref().is_some_and(|w| w.changed.load(Ordering::Relaxed)), "the change waits for a frame");
        refresh(&mut h.app);
        assert!(h.app.plugin_slots.discovery.is_some(), "a changed folder is read again");
        wait_for_discovery(&mut h.app);
        assert!(h.app.plugin_slots.watch.as_ref().is_some_and(|w| !w.changed.load(Ordering::Relaxed)), "the change was taken");
    }

    fn wait_until(mut done: impl FnMut() -> bool) {
        for _ in 0..400 {
            if done() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(done(), "timed out");
    }
}
