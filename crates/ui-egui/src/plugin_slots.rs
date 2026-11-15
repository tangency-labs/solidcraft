//! Manifest placements drawn by the host, opening the Plugins menu entry dialog.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::{SolveApp, plugins, ui_kit};
#[cfg(not(target_arch = "wasm32"))]
use solidcraft_engine::plugin::UiSlot;

#[derive(Clone)]
pub(crate) struct Item {
    pub command: String,
    pub plugin: String,
    pub entry: String,
    pub label: String,
    pub slot: Slot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Menu(&'static str),
    Ribbon(&'static str),
}
#[derive(Default)]
pub(crate) struct State {
    items: Vec<Item>,
    scanned: Option<f64>,
    discovery: Option<Receiver<Inventory>>,
    inventory: Option<Inventory>,
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
impl State {
    pub(crate) fn discovery_pending(&self) -> bool {
        self.discovery.is_some()
    }
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
    app.plugin_slots.inventory = Some(inventory);
}
pub(crate) fn refresh(app: &mut SolveApp) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    let now = crate::now_ms();
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
    if app.plugin_slots.scanned.is_some_and(|at| now - at < 1000.0) {
        return;
    }
    let root = app.session.plugin_state.plugins_dir();
    let inspected = app.session.plugin_state.packages.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    #[cfg(not(target_arch = "wasm32"))]
    if std::thread::Builder::new()
        .name("plugin-discovery".into())
        .spawn(move || {
            let (plugins, skipped) = plugins::inventory(root.as_deref(), inspected.values());
            let _ = tx.send(Inventory { root, plugins, skipped });
        })
        .is_err()
    {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (root, inspected, tx);
    app.plugin_slots.discovery = Some(rx);
    app.plugin_slots.scanned = Some(now);
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
        let row = ui_kit::Item::action(&item.command, &item.label, "prefs_plugins");
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
                        if ui_kit::menu::list_row(ui, &ui_kit::Item::action(&item.command, &item.label, "prefs_plugins")).is_some() {
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
        let slot = h.app.plugin_menu.dialog.take().unwrap();
        assert!(slot.ready(&h.app).unwrap_err().contains("file"));
        assert!(h.app.control_held.is_empty(), "missing grants never queue a run");
        crate::plugin_menu::open_entry(&mut h.app, 0, 0);
        let menu = h.app.plugin_menu.dialog.as_ref().unwrap();
        assert_eq!(slot.fields, menu.fields);
        assert_eq!(slot.plugin.approval, menu.plugin.approval);
        assert_eq!(slot.ready(&h.app), menu.ready(&h.app));
        h.app.session.plugin_state.grants.entry("helper".into()).or_default().command_groups.insert("file".into());
        assert_eq!(menu.ready(&h.app), Ok(json!({"count":3})));
        h.app.preferences.disabled_plugins.insert("helper".into());
        assert!(menu.ready(&h.app).is_err(), "disabling an already open dialog prevents running it");
        h.app.plugin_menu.dialog = None;
        assert!(items(&h.app, Slot::Menu("Help")).is_empty());
        begin(&mut h.app, "helper.report");
        assert!(h.app.plugin_menu.dialog.is_none());
        h.app.preferences.disabled_plugins.clear();
        for command in ["helper.missing", "missing.report", "solid.box", "helper.report.extra"] {
            begin(&mut h.app, command);
            assert!(h.app.plugin_menu.dialog.is_none(), "{command}");
        }
        std::fs::remove_file(dir.join("plugin.wasm")).unwrap();
        begin(&mut h.app, "helper.report");
        assert!(h.app.plugin_menu.dialog.is_none(), "a package the shared reader refuses cannot open a slot dialog");
    }
}
