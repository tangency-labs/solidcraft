//! Manifest placements drawn by the host, opening the Plugins menu entry dialog.
use std::path::{Path, PathBuf};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{RecvTimeoutError, Sender, TryRecvError};

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
    /// The [`signature`] of what the latest scan read, taken by the scan's worker before it reads:
    /// the watcher compares the folder with this, so a change made while a scan runs, or before
    /// the watcher's first look, is never taken as the settled state.
    #[cfg(not(target_arch = "wasm32"))]
    scanned_signature: Arc<AtomicU64>,
}

/// What the watcher and the scan read: the plugins' directory and the packages inspected from
/// elsewhere (`plugin.inspect`), which the scan reads as well.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, PartialEq)]
struct Watched {
    root: Option<PathBuf>,
    inspected: Vec<PathBuf>,
}

/// A thread that looks at the plugins' directory every [`WATCH_MS`], and asks for a frame only
/// when what it holds changed: the alternative to rescanning the folder on a timer.
#[cfg(not(target_arch = "wasm32"))]
struct Watch {
    watched: Watched,
    changed: Arc<AtomicBool>,
    /// Dropped with the window (or when another root is watched): the thread's wait ends at once
    /// and it stops, without the window waiting for it.
    _stop: Sender<()>,
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

#[cfg(any(test, not(target_arch = "wasm32")))]
fn discovered(app: &mut SolveApp, inventory: Inventory) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.plugin_slots.items = inventory.plugins.iter().flat_map(declared).take(8192).collect();
    }
    let root = inventory.root.clone();
    // The Plugins menu and the Model tab's ribbon list what this scan found, once, here: no frame
    // reads the plugin folders itself (#21594).
    #[cfg(not(target_arch = "wasm32"))]
    if !app.plugin_menu.loaded {
        crate::plugin_menu::adopt(app, &inventory.plugins);
    }
    app.plugin_slots.inventory = Some(inventory);
    app.plugin_slots.scanned = Some((crate::now_ms(), root));
}
pub(crate) fn refresh(app: &mut SolveApp) {
    #[cfg(target_arch = "wasm32")]
    let _ = app;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let root = app.session.plugin_state.plugins_dir();
        let inspected = app.session.plugin_state.packages.values().map(PathBuf::from).collect();
        start_watch(app, Watched { root: root.clone(), inspected }, std::time::Duration::from_millis(WATCH_MS));
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
    let scanned = app.plugin_slots.scanned_signature.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("plugin-discovery".into()).spawn(move || {
        // Before the read: a change made during it differs from this, so the watcher asks again.
        let watched = Watched { root: root.clone(), inspected: inspected.values().map(PathBuf::from).collect() };
        scanned.store(signature(&watched), Ordering::Relaxed);
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

/// Watch the plugins' directory (and inspected packages) for changes, once per set: a plugin
/// dropped in or edited while the app runs asks for a frame, which rescans the folder. Nothing is
/// read while it stays the same.
#[cfg(not(target_arch = "wasm32"))]
fn start_watch(app: &mut SolveApp, watched: Watched, every: std::time::Duration) {
    let Some(ctx) = app.plugin_slots.ctx.clone() else { return };
    if app.plugin_slots.watch.as_ref().is_some_and(|w| w.watched == watched) {
        return;
    }
    // A watcher that cannot start is harmless: the folder is read when the design opens and when
    // Preferences or the Plugins menu ask, and a restart finds a plugin dropped in meanwhile.
    let scanned = app.plugin_slots.scanned_signature.clone();
    if let Some(watch) = spawn_watch(watched, every, scanned, move || ctx.request_repaint()) {
        app.plugin_slots.watch = Some(watch);
    }
}

/// The watcher's thread: every `every` it compares what it watches with what the latest scan read
/// (`scanned`), and wakes the window once for each new state that differs. It never takes a
/// state as settled by itself, so nothing that changed around a scan is missed.
#[cfg(not(target_arch = "wasm32"))]
fn spawn_watch(watched: Watched, every: std::time::Duration, scanned: Arc<AtomicU64>, wake: impl Fn() + Send + 'static) -> Option<Watch> {
    let changed = Arc::new(AtomicBool::new(false));
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let (flag, looked_at) = (changed.clone(), watched.clone());
    let spawned = std::thread::Builder::new().name("plugin-folder".into()).spawn(move || {
        let mut woken_for = None;
        // Nothing is ever sent: the wait times out between looks, and ends when the Watch drops.
        while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(every) {
            if should_wake(&mut woken_for, signature(&looked_at), scanned.load(Ordering::Relaxed)) {
                flag.store(true, Ordering::Relaxed);
                wake();
            }
        }
    });
    spawned.ok().map(|_| Watch { watched, changed, _stop: stop })
}

/// Whether the watcher wakes the window: the folder (`now`) differs from what the latest scan read
/// (`scanned`), and it has not already woken it for this folder against this scan. The scan is
/// part of the memory, so after another scan reads something else the same folder wakes it again
/// (the folder going B, scanned, A, scanned, back to B is a change to report).
#[cfg(not(target_arch = "wasm32"))]
fn should_wake(woken_for: &mut Option<(u64, u64)>, now: u64, scanned: u64) -> bool {
    if now == scanned || *woken_for == Some((now, scanned)) {
        return false;
    }
    *woken_for = Some((now, scanned));
    true
}

/// What the plugins' directory and the inspected packages hold, cheaply: whether the directory
/// exists, each entry's name, size and modification time, and for each plugin folder its
/// `plugin.json` and what else it holds directly (its images), because rewriting a file in place
/// changes no folder's time. A dropped-in, replaced, edited or removed plugin changes it; nothing
/// else does. Symbolic links are described as links, as the scan reads them (it skips a linked
/// folder or manifest).
#[cfg(not(target_arch = "wasm32"))]
fn signature(watched: &Watched) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let dir = watched.root.as_deref();
    dir.is_some_and(Path::exists).hash(&mut h);
    let mut items: Vec<(std::ffi::OsString, Option<Described>, Vec<Described>)> = Vec::new();
    if let Some(entries) = dir.and_then(|dir| std::fs::read_dir(dir).ok()) {
        // The scan reads at most 1024 folders; files and hidden entries come on top of those.
        for top in entries.flatten().take(4096) {
            let path = top.path();
            let own = describe(&path);
            let inner = if own.as_ref().is_some_and(|d| d.3) { plugin_folder(&path) } else { Vec::new() };
            items.push((top.file_name(), own, inner));
        }
    }
    items.sort();
    items.hash(&mut h);
    for package in &watched.inspected {
        (describe(package), plugin_folder(package)).hash(&mut h);
    }
    h.finish()
}

/// A name, size, modification time and whether it is a folder, without following a link.
#[cfg(not(target_arch = "wasm32"))]
type Described = (std::ffi::OsString, u64, u128, bool);

#[cfg(not(target_arch = "wasm32"))]
fn describe(path: &Path) -> Option<Described> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    Some((path.file_name().map(std::ffi::OsStr::to_os_string).unwrap_or_default(), meta.len(), modified, meta.is_dir()))
}

/// A plugin folder's `plugin.json` always, then up to 256 of the other entries it holds directly,
/// so a folder with very many files neither hides its manifest nor costs much to look at.
#[cfg(not(target_arch = "wasm32"))]
fn plugin_folder(dir: &Path) -> Vec<Described> {
    let mut inner: Vec<Described> = std::fs::read_dir(dir)
        .map(|children| children.flatten().filter(|c| c.file_name() != "plugin.json").take(256).filter_map(|c| describe(&c.path())).collect())
        .unwrap_or_default();
    inner.sort();
    inner.extend(describe(&dir.join("plugin.json")));
    inner
}
pub(crate) fn items(app: &SolveApp, slot: Slot) -> Vec<Item> {
    app.plugin_slots
        .items
        .iter()
        .filter(|item| item.slot == slot && item.command.split_once('.').is_some_and(|(id, _)| plugins::enabled(app, id)))
        .cloned()
        .collect()
}

/// The File menu's Export rows for the installed, enabled plugins' export slots (RFC 0015 §4):
/// each slot's `plugin_id.entry` and its row label, "Export <label> (.ext)…", where the label is
/// the slot's own or the plugin's name. From the discovered inventory: no file is read here.
pub(crate) fn export_slots(app: &SolveApp) -> Vec<(String, String)> {
    let Some(inventory) = app.plugin_slots.inventory.as_ref() else { return Vec::new() };
    let mut rows = Vec::new();
    for plugin in inventory.plugins.iter().filter(|p| plugins::enabled(app, &p.id)) {
        for entry in plugin.entries.iter().filter(|e| e.kind == solidcraft_engine::plugin::EntryKind::Exporter) {
            let label = if entry.label.is_empty() { plugin.name.as_str() } else { entry.label.as_str() };
            let extensions = entry.extensions.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(", ");
            let label = solidcraft_ui_text::format("plugin_export.menu", &[("format", label), ("extensions", &extensions)]);
            rows.push((format!("{}.{}", plugin.id, entry.name), label));
        }
    }
    rows
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
            ribbon_groups: Vec::new(),
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
        let h = crate::scenario::Harness::new();
        let dir = h.dir.join("installed");
        assert_eq!(sig(&dir), sig(&dir), "a missing folder is stable");
        std::fs::create_dir_all(&dir).unwrap();
        let empty = sig(&dir);
        assert_ne!(empty, sig(&dir.join("absent")), "a folder appearing is a change");
        assert_eq!(empty, sig(&dir), "an empty folder is stable");
        std::fs::create_dir_all(dir.join("helper")).unwrap();
        assert_ne!(empty, sig(&dir), "a dropped-in plugin folder is a change");
        let folder = sig(&dir);
        std::fs::write(dir.join("helper").join("plugin.json"), b"{}").unwrap();
        assert_ne!(folder, sig(&dir), "its manifest arriving is a change");
        let with_manifest = sig(&dir);
        assert_eq!(with_manifest, sig(&dir), "a settled plugin is stable");
        // Rewritten in place (same file, no rename), so the plugin folder's own time stays put.
        std::fs::write(dir.join("helper").join("plugin.json"), b"{\"id\":\"helper\"}").unwrap();
        assert_ne!(with_manifest, sig(&dir), "an edited manifest is a change");
        std::fs::remove_dir_all(dir.join("helper")).unwrap();
        assert_eq!(empty, sig(&dir), "removing it is a change back");
        let none = Watched { root: None, inspected: Vec::new() };
        assert_eq!(signature(&none), signature(&none), "no folder at all is stable");
        // A package inspected from elsewhere is read by the scan too, so its edits count.
        let package = h.dir.join("elsewhere");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("plugin.json"), b"{}").unwrap();
        let inspected = Watched { root: Some(dir.clone()), inspected: vec![package.clone()] };
        let before = signature(&inspected);
        assert_eq!(before, signature(&inspected), "a settled inspected package is stable");
        std::fs::write(package.join("plugin.json"), b"{\"id\":\"elsewhere\"}").unwrap();
        assert_ne!(before, signature(&inspected), "an inspected package's edited manifest is a change");
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
        // The worker asks for its frame just after it sends the result.
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
        invalidate(&mut h.app);
        refresh(&mut h.app);
        wait_for_discovery(&mut h.app);
        // A short look interval, so the test does not wait 2 s; the frame's own `refresh` keeps
        // this watcher (it watches the same).
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = woken.clone();
        let inspected = h.app.session.plugin_state.packages.values().map(PathBuf::from).collect();
        let watched = Watched { root: Some(dir.clone()), inspected };
        let scanned = h.app.plugin_slots.scanned_signature.clone();
        let wake = move || {
            count.fetch_add(1, Ordering::Relaxed);
        };
        h.app.plugin_slots.watch = spawn_watch(watched, std::time::Duration::from_millis(10), scanned, wake);
        assert!(h.app.plugin_slots.watch.is_some(), "the watcher started");
        std::fs::create_dir_all(dir.join("helper")).unwrap();
        wait_until(|| woken.load(Ordering::Relaxed) >= 1);
        assert!(h.app.plugin_slots.watch.as_ref().is_some_and(|w| w.changed.load(Ordering::Relaxed)), "the change waits for a frame");
        refresh(&mut h.app);
        assert!(h.app.plugin_slots.discovery.is_some(), "a changed folder is read again");
        wait_for_discovery(&mut h.app);
        assert!(h.app.plugin_slots.watch.as_ref().is_some_and(|w| !w.changed.load(Ordering::Relaxed)), "the change was taken");
        assert_eq!(woken.load(Ordering::Relaxed), 1, "one change wakes the window once");
    }

    /// The watcher compares the folder with what the latest scan read, not with its own first
    /// look: a plugin added after the scan read the folder but before the watcher's first look
    /// still wakes the window.
    #[test]
    fn a_plugin_added_before_the_watchers_first_look_is_not_missed() {
        let h = crate::scenario::Harness::new();
        let dir = h.dir.join("installed");
        std::fs::create_dir_all(&dir).unwrap();
        let watched = Watched { root: Some(dir.clone()), inspected: Vec::new() };
        let scanned = Arc::new(AtomicU64::new(signature(&watched)));
        std::fs::create_dir_all(dir.join("helper")).unwrap();
        let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = woken.clone();
        let watch = spawn_watch(watched.clone(), std::time::Duration::from_millis(10), scanned, move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
        assert!(watch.is_some(), "the watcher started");
        wait_until(|| woken.load(Ordering::Relaxed) >= 1);
        assert!(watch.as_ref().is_some_and(|w| w.changed.load(Ordering::Relaxed)), "the plugin asks for a scan");
    }

    /// Polls a condition another thread makes true. The limit only bounds a failing run, so it is
    /// generous: a slow CI runner waits longer, it never fails a passing run.
    /// The watcher wakes once per new folder state, and again for a state it already reported
    /// when a scan has since read something else: B is reported, a scan reads B, the folder
    /// becomes A and an invalidation scans A before the watcher looks, then the folder returns to
    /// exactly B. The last scan read A, so B must wake the window again.
    #[test]
    fn a_folder_returning_to_a_reported_state_after_another_scan_wakes_again() {
        let (a, b, start) = (1_u64, 2_u64, 0_u64);
        let mut woken_for = None;
        assert!(should_wake(&mut woken_for, b, start), "B differs from the first scan");
        assert!(!should_wake(&mut woken_for, b, start), "and wakes once while its scan is pending");
        assert!(!should_wake(&mut woken_for, b, b), "the scan read B: nothing to say");
        // The folder became A and an invalidation scanned it before the watcher looked.
        assert!(!should_wake(&mut woken_for, a, a), "the scan read A: nothing to say");
        assert!(should_wake(&mut woken_for, b, a), "back to B after a scan of A wakes the window");
        assert!(!should_wake(&mut woken_for, b, a), "once");
    }

    fn sig(dir: &Path) -> u64 {
        signature(&Watched { root: Some(dir.to_path_buf()), inspected: Vec::new() })
    }

    fn wait_until(mut done: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !done() {
            assert!(std::time::Instant::now() < deadline, "timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// #21228, #21594: once the installed plugins have been read, drawing frames (the Model tab's
    /// ribbon, the app bar, the viewport) reads and hashes no plugin file. Only a change reads
    /// them again, once, in the background.
    #[test]
    fn steady_state_frames_read_no_plugin_files() {
        let mut h = crate::scenario::Harness::new();
        let root = h.dir.join("plugins-steady");
        let package = root.join("big-importer");
        std::fs::create_dir_all(&package).unwrap();
        // A component big enough that hashing it every frame would show in the time of a frame.
        std::fs::write(package.join("plugin.wasm"), vec![7u8; 8 << 20]).unwrap();
        let manifest = json!({
            "id":"big-importer", "name":"Big importer", "version":"1", "capabilities":["commands@0.1"],
            "runtime":{"module":"plugin.wasm","commands":[],
                "entries":[{"name":"report","effect":"read","slots":[{"kind":"menu","name":"Help"}]}]}
        });
        std::fs::write(package.join("plugin.json"), manifest.to_string()).unwrap();
        h.app.session.plugin_state.plugins_dir = Some(root.clone());
        invalidate(&mut h.app);
        let settle = |h: &mut crate::scenario::Harness| {
            for _ in 0..2000 {
                h.frame();
                let scanned_here = h.app.plugin_slots.scanned.as_ref().is_some_and(|(_, at)| at.as_deref() == Some(root.as_path()));
                if scanned_here && h.app.plugin_slots.discovery.is_none() {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("discovery never finished");
        };
        settle(&mut h);
        let reads = plugins::inventory_reads(&root);
        assert!(reads >= 1, "the first frames read the installed plugins");
        assert_eq!(h.app.plugin_menu.listed.len(), 1, "the ribbon and menus list what discovery found");
        assert_eq!(items(&h.app, Slot::Menu("Help")).len(), 1);
        h.frames(150);
        assert_eq!(plugins::inventory_reads(&root), reads, "steady-state frames read no plugin file");
        assert!(h.app.plugin_slots.discovery.is_none());

        // A change reads them again, once.
        invalidate(&mut h.app);
        settle(&mut h);
        h.frames(150);
        assert_eq!(plugins::inventory_reads(&root), reads + 1);
    }
}
