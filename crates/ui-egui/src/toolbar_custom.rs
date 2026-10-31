//! Toolbar customisation: which commands each panel shows as buttons, per workspace and tab.
//! A command's menu (right-click on a toolbar button, a panel drop-down row or an S box row)
//! pins it to the toolbar, removes it, or pins it to the S box's favourites; buttons are dragged
//! to reorder them within and between panels; a drop-down row dragged onto the toolbar pins it
//! there, a button dragged off the toolbar (or back into the drop-down) leaves it; the pin icon
//! on a hovered drop-down row pins or unpins; the toolbar's own menu (the plus after the tabs,
//! or right-click its empty space or a tab) shows or hides each tab, and has Customize Toolbar…
//! (a window listing every command of the tab with a check box) and Reset Toolbar; Reset Toolbar
//! restores the tab. The buttons are kept with the preferences and exported or imported with the
//! keyboard shortcuts; hidden tabs are kept in the `hidden_toolbar_tabs` preference.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_engine::{CommandSpec, command_specs};

use crate::context_menu::Item;
use crate::{SolveApp, ui_kit};

/// The only workspace so far.
pub const WORKSPACE: &str = "DESIGN";

#[derive(Default, Clone, Debug, PartialEq)]
pub struct ToolbarCustom {
    /// `"<workspace>/<tab>/<panel>"` → the panel's buttons, in order (only panels the user
    /// changed).
    pub panels: BTreeMap<String, Vec<String>>,
    /// `"<workspace>/<tab>"` → gallery commands moved to full-size buttons.
    pub gallery_buttons: BTreeMap<String, Vec<String>>,
    /// `"<workspace>/<tab>/<panel>"` → the order of the panel's drop-down, as the user dragged
    /// it (#5080; only menus the user changed).
    pub menus: BTreeMap<String, Vec<String>>,
    /// The Customize Toolbar window is open (not kept).
    pub customizing: bool,
}

/// Lists of command ids by panel key, as saved: at most 500 lists of 100 ids.
fn id_lists(v: &Value) -> BTreeMap<String, Vec<String>> {
    v.as_object()
        .map(|o| {
            o.iter()
                .take(500)
                .filter_map(|(k, v)| {
                    let ids: Vec<String> = v.as_array()?.iter().take(100).filter_map(|x| x.as_str().map(str::to_string)).collect();
                    Some((k.clone(), ids))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl ToolbarCustom {
    /// Whether a command still uses a compact gallery cell in this panel.
    pub fn in_gallery(&self, tab: &str, panel: &str, id: &str) -> bool {
        tab == "SKETCH"
            && panel == "CONSTRAIN"
            && crate::workspace::CONSTRAINT_GALLERY.contains(&id)
            && !self.gallery_buttons.get(&format!("{WORKSPACE}/{tab}")).is_some_and(|ids| ids.iter().any(|c| c == id))
    }

    pub fn prefs(&self) -> Value {
        let mut v = json!({"version": 3, "panels": self.panels, "gallery_buttons": self.gallery_buttons});
        // Saved only once a menu was reordered, so other settings stay as they were.
        if !self.menus.is_empty() {
            v["menus"] = json!(self.menus);
        }
        v
    }
    pub fn load(&mut self, v: &Value) {
        let version = v.get("version").and_then(Value::as_u64).unwrap_or(0);
        let current = version >= 2;
        let panels = if current { v.get("panels").unwrap_or(&Value::Null) } else { v };
        self.panels = id_lists(panels);
        self.gallery_buttons = id_lists(v.get("gallery_buttons").unwrap_or(&Value::Null));
        self.menus = if current { id_lists(v.get("menus").unwrap_or(&Value::Null)) } else { BTreeMap::new() };
        if !current {
            self.migrate();
        }
        if version < 3 {
            self.migrate_render_tabs();
        }
    }
    /// Split the former six-section Render ribbon without losing personal pins or removals.
    fn migrate_render_tabs(&mut self) {
        let mut migrated_panels: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (old_tab, old, targets) in [
            ("RENDER", "SCENE", &[("RENDER CAMERA", "SHOTS"), ("RENDER CAMERA", "VIEW"), ("RENDER SCENE", "STUDIO")][..]),
            ("RENDER", "APPEARANCE", &[("RENDER APPEARANCE", "APPEARANCE")][..]),
            ("RENDER", "LIGHTING", &[("RENDER SCENE", "STUDIO")][..]),
            ("RENDER", "ENVIRONMENT", &[("RENDER SCENE", "STAGE")][..]),
            ("RENDER", "CAMERA", &[("RENDER CAMERA", "LENS"), ("RENDER CAMERA", "FRAME"), ("RENDER CAMERA", "VIEW")][..]),
            ("RENDER", "IMAGE", &[("RENDER", "IMAGE"), ("RENDER", "RENDER"), ("RENDER", "RESULT")][..]),
            ("RENDER LIGHTING", "LIGHTING", &[("RENDER SCENE", "STUDIO"), ("RENDER SCENE", "LIGHTS")][..]),
            ("RENDER LIGHTING", "ENVIRONMENT", &[("RENDER SCENE", "STAGE"), ("RENDER SCENE", "STUDIO")][..]),
            ("RENDER", "OUTPUT", &[("RENDER", "RENDER"), ("RENDER", "IMAGE")][..]),
            ("RENDER", "RESULTS", &[("RENDER", "RESULT"), ("RENDER", "IMAGE")][..]),
        ] {
            let Some(ids) = self.panels.remove(&key(old_tab, old)) else { continue };
            for &(tab, panel) in targets {
                let k = key(tab, panel);
                let migrated: Vec<_> = ids
                    .iter()
                    .filter(|id| solidcraft_engine::find_command(id).and_then(crate::workspace::legacy_render_destination) == Some((tab, panel)))
                    .cloned()
                    .collect();
                let list = migrated_panels.entry(k).or_default();
                for id in migrated {
                    if !list.contains(&id) {
                        list.push(id);
                    }
                }
            }
            // Preserve deliberately cross-pinned commands with no destination in this split.
            if let Some(&(tab, panel)) = targets.first() {
                let list = migrated_panels.entry(key(tab, panel)).or_default();
                for id in ids {
                    let destination = solidcraft_engine::find_command(&id).and_then(crate::workspace::legacy_render_destination);
                    if !targets.iter().any(|target| Some(*target) == destination) && !list.contains(&id) {
                        list.push(id);
                    }
                }
            }
        }
        for (k, ids) in migrated_panels {
            // New-layout customisation is authoritative when both versions are present.
            self.panels.entry(k).or_insert(ids);
        }
    }

    fn migrate(&mut self) {
        use std::collections::BTreeSet;
        let specs = command_specs();
        let mut pins: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut removals: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let old = std::mem::take(&mut self.panels);
        for (k, ids) in old {
            let parts: Vec<_> = k.split('/').collect();
            let ["DESIGN", tab, panel] = parts.as_slice() else {
                self.panels.insert(k, ids);
                continue;
            };
            if crate::workspace::legacy_panel(tab, panel).is_none() {
                self.panels.insert(k, ids);
                continue;
            }
            if let Some((t, p)) = crate::workspace::legacy_panel(tab, panel) {
                pins.entry(key(t, p)).or_default();
            }
            for id in crate::workspace::legacy_buttons(tab, panel) {
                if !ids.iter().any(|x| x == id)
                    && let Some((t, p)) = crate::workspace::legacy_destination(tab, panel, id)
                {
                    removals.entry(key(t, p)).or_default().insert(id.into());
                }
            }
            for id in ids {
                if let Some((t, p)) = crate::workspace::legacy_destination(tab, panel, &id) {
                    let list = pins.entry(key(t, p)).or_default();
                    if !list.contains(&id) {
                        list.push(id);
                    }
                }
            }
        }
        let touched: BTreeSet<_> = pins.keys().chain(removals.keys()).cloned().collect();
        for k in touched {
            if self.panels.contains_key(&k) {
                continue;
            } // An existing new destination wins.
            let mut ids = pins.remove(&k).unwrap_or_default();
            let parts: Vec<_> = k.split('/').collect();
            if let [_, tab, panel] = parts.as_slice()
                && let Some((cmds, n)) = crate::workspace::layout(tab, panel, &specs)
            {
                for c in cmds.iter().take(n) {
                    if !ids.iter().any(|id| id == c.id) && !removals.get(&k).is_some_and(|r| r.contains(c.id)) {
                        ids.push(c.id.into());
                    }
                }
            }
            self.panels.insert(k, ids);
        }
    }
}

fn key(tab: &str, panel: &str) -> String {
    format!("{WORKSPACE}/{tab}/{panel}")
}

/// The buttons a panel shows: the user's list, or the first `promoted` commands of the panel.
pub fn buttons(app: &SolveApp, tab: &str, panel: &str, cmds: &[&'static CommandSpec], promoted: usize) -> Vec<&'static CommandSpec> {
    match app.toolbar_custom.panels.get(&key(tab, panel)) {
        Some(ids) => ids.iter().filter_map(|id| solidcraft_engine::find_command(id)).collect(),
        None => cmds.iter().take(promoted).copied().collect(),
    }
}

/// A tab's panels with their commands and default button counts.
pub type PanelLists = Vec<(&'static str, Vec<&'static CommandSpec>, usize)>;

/// The panels of a tab with their commands and default button counts (what the toolbar draws).
/// Made from the command registry and the ribbon's layout, which never change while the app
/// runs, so each tab's lists are worked out once: the toolbar asks for them every frame, and
/// working them out matches every command against every section (about a millisecond a frame).
pub fn panel_lists(tab: &str) -> PanelLists {
    static LISTS: std::sync::Mutex<BTreeMap<String, PanelLists>> = std::sync::Mutex::new(BTreeMap::new());
    let Ok(mut lists) = LISTS.lock() else { return panel_lists_of(tab) };
    lists.entry(tab.to_string()).or_insert_with(|| panel_lists_of(tab)).clone()
}

/// [`panel_lists`] worked out from the registry.
fn panel_lists_of(tab: &str) -> PanelLists {
    let specs = command_specs();
    crate::toolbar::panels(tab)
        .iter()
        .filter_map(|panel| {
            let (cmds, n) = crate::workspace::layout(tab, panel, &specs)?;
            (!cmds.is_empty() || *panel == "SELECT").then_some((*panel, cmds, n))
        })
        .collect()
}

/// The buttons of a panel as ids, made the user's own list (so it can change).
fn own(app: &mut SolveApp, tab: &str, panel: &str) -> Vec<String> {
    let k = key(tab, panel);
    if let Some(v) = app.toolbar_custom.panels.get(&k) {
        return v.clone();
    }
    let lists = panel_lists(tab);
    let ids: Vec<String> =
        lists.iter().find(|(p, ..)| *p == panel).map(|(_, cmds, n)| cmds.iter().take(*n).map(|c| c.id.to_string()).collect()).unwrap_or_default();
    ids
}

/// Where a command is a button on a tab now: its panel.
pub fn pinned_on(app: &SolveApp, tab: &str, id: &str) -> Option<&'static str> {
    panel_lists(tab).into_iter().find(|(panel, cmds, n)| buttons(app, tab, panel, cmds, *n).iter().any(|c| c.id == id)).map(|(p, ..)| p)
}

/// The panel a command belongs to on a tab (its drop-down), or the tab's first panel.
fn home_panel(tab: &str, id: &str) -> Option<&'static str> {
    let lists = panel_lists(tab);
    lists.iter().find(|(_, cmds, _)| cmds.iter().any(|c| c.id == id)).or(lists.first()).map(|(p, ..)| *p)
}

/// FINISH EDITING, shown only while a linked design is edited in place, keeps its one button:
/// nothing moves into it, and Finish Edit moves nowhere else.
fn fixed(tab: &str, panel: &str, id: &str) -> bool {
    (tab == "MODEL" && panel == "FINISH EDITING") || id == "component.finish_edit"
}

/// Pin a command to the toolbar of a tab (as a button of its panel, at the end).
pub fn pin(app: &mut SolveApp, tab: &str, id: &str) -> Result<(), String> {
    if solidcraft_engine::find_command(id).is_none() {
        return Err(format!("no command `{id}`"));
    }
    if pinned_on(app, tab, id).is_some() || fixed(tab, "", id) {
        return Ok(());
    }
    let panel = home_panel(tab, id).ok_or_else(|| format!("the {tab} tab has no panels"))?;
    let mut ids = own(app, tab, panel);
    ids.push(id.to_string());
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
    Ok(())
}

/// Remove a command's button from a tab (it stays in its panel's drop-down).
pub fn remove(app: &mut SolveApp, tab: &str, id: &str) {
    let Some(panel) = pinned_on(app, tab, id).filter(|panel| !fixed(tab, panel, id)) else { return };
    let mut ids = own(app, tab, panel);
    ids.retain(|x| x != id);
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
}

/// Move a button before another one (`before`: None puts it at the end of `panel`).
pub fn reorder(app: &mut SolveApp, tab: &str, id: &str, panel: &str, before: Option<&str>) {
    if fixed(tab, panel, id) {
        return;
    }
    // A gallery command dropped on a gallery cell goes (back) into the gallery; dropped anywhere
    // else it becomes a full-size button.
    if tab == "SKETCH" && crate::workspace::CONSTRAINT_GALLERY.contains(&id) {
        let into_gallery = before.is_some_and(|b| app.toolbar_custom.in_gallery(tab, panel, b));
        let ids = app.toolbar_custom.gallery_buttons.entry(format!("{WORKSPACE}/{tab}")).or_default();
        ids.retain(|c| c != id);
        if !into_gallery {
            ids.push(id.to_string());
        }
    }
    if let Some(from) = pinned_on(app, tab, id) {
        let mut ids = own(app, tab, from);
        ids.retain(|x| x != id);
        app.toolbar_custom.panels.insert(key(tab, from), ids);
    }
    let mut ids = own(app, tab, panel);
    ids.retain(|x| x != id);
    let at = before.and_then(|b| ids.iter().position(|x| x == b)).unwrap_or(ids.len());
    ids.insert(at, id.to_string());
    app.toolbar_custom.panels.insert(key(tab, panel), ids);
}

/// The pin icon of a drop-down row: pin the command, or remove it when it is pinned.
pub fn toggle(app: &mut SolveApp, tab: &str, id: &str) {
    if pinned_on(app, tab, id).is_some() {
        remove(app, tab, id);
    } else if let Err(e) = pin(app, tab, id) {
        app.set_status(e, true);
    }
}

/// Reset Toolbar: a tab's panels, and the order of their drop-downs, back to their defaults.
pub fn reset(app: &mut SolveApp, tab: &str) {
    let prefix = format!("{WORKSPACE}/{tab}/");
    app.toolbar_custom.panels.retain(|k, _| !k.starts_with(&prefix));
    app.toolbar_custom.gallery_buttons.remove(&format!("{WORKSPACE}/{tab}"));
    app.toolbar_custom.menus.retain(|k, _| !k.starts_with(&prefix));
}

/// A drop-down's commands (as `workspace::grouped_commands` lists them) in the user's order
/// (#5080). Within each group, the commands the saved order names take its order among the
/// places they hold; any other (a command added since) keeps its place. The group's heading
/// stays on its first command.
pub fn order_grouped<'a>(
    app: &SolveApp,
    tab: &str,
    panel: &str,
    grouped: Vec<(Option<&'static str>, &'a CommandSpec)>,
) -> Vec<(Option<&'static str>, &'a CommandSpec)> {
    let Some(saved) = app.toolbar_custom.menus.get(&key(tab, panel)) else { return grouped };
    let groups = crate::workspace::groups(tab, panel);
    let group_of = |id: &str| groups.iter().position(|g| g.ids.contains(&id));
    let rank = |id: &str| saved.iter().position(|s| s == id);
    let mut out = grouped;
    let mut start = 0;
    while start < out.len() {
        let group = out.get(start).map(|(_, c)| group_of(c.id)).unwrap_or_default();
        let end = (start..out.len()).find(|i| out.get(*i).is_some_and(|(_, c)| group_of(c.id) != group)).unwrap_or(out.len());
        let heading = out.get(start).and_then(|(h, _)| *h);
        let Some(run) = out.get_mut(start..end) else { break };
        let slots: Vec<usize> = (0..run.len()).filter(|i| run.get(*i).is_some_and(|(_, c)| rank(c.id).is_some())).collect();
        let mut named: Vec<_> = slots.iter().filter_map(|i| run.get(*i).map(|(_, c)| *c)).collect();
        named.sort_by_key(|c| rank(c.id));
        for (slot, c) in slots.iter().zip(named) {
            if let Some(entry) = run.get_mut(*slot) {
                entry.1 = c;
            }
        }
        for (i, entry) in run.iter_mut().enumerate() {
            entry.0 = if i == 0 { heading } else { None };
        }
        start = end;
    }
    out
}

/// Move a command in its panel's drop-down before `before` (None: to the end), within its own
/// group: a place in another group moves it to the nearest end of its own (#5080).
pub fn move_in_menu(app: &mut SolveApp, tab: &str, panel: &str, id: &str, before: Option<&str>) {
    let lists = panel_lists(tab);
    let Some((_, cmds, _)) = lists.iter().find(|(p, ..)| *p == panel) else { return };
    let order: Vec<&str> = order_grouped(app, tab, panel, crate::workspace::grouped_commands(tab, panel, cmds)).iter().map(|(_, c)| c.id).collect();
    if !order.contains(&id) || before == Some(id) {
        return;
    }
    let groups = crate::workspace::groups(tab, panel);
    let group_of = |x: &str| groups.iter().position(|g| g.ids.contains(&x)).unwrap_or(groups.len());
    let own = group_of(id);
    let mut ids: Vec<&str> = order.into_iter().filter(|x| *x != id).collect();
    let first = ids.iter().position(|x| group_of(x) == own);
    let after_last = ids.iter().rposition(|x| group_of(x) == own).map(|i| i + 1);
    let end_of_group = after_last.or(first).unwrap_or_else(|| ids.iter().position(|x| group_of(x) > own).unwrap_or(ids.len()));
    let at = match before.and_then(|b| ids.iter().position(|x| *x == b).map(|i| (i, group_of(b)))) {
        Some((i, g)) if g == own => i,
        Some((_, g)) if g < own => first.unwrap_or(end_of_group),
        _ => end_of_group,
    };
    ids.insert(at.min(ids.len()), id);
    app.toolbar_custom.menus.insert(key(tab, panel), ids.into_iter().map(str::to_string).collect());
}

/// The menu of a command (toolbar button, drop-down row, S box row).
pub fn command_items(app: &SolveApp, id: &str) -> Vec<Item> {
    let tab = app.ui.tab.clone();
    let on = pinned_on(app, &tab, id).is_some();
    let fav = app.sbox.pinned.iter().any(|p| p == id);
    let mut v = Vec::new();
    v.push(if on {
        Item::action("ui.toolbarRemove", "Remove from Toolbar", "close").with(json!({ "command": id }))
    } else {
        Item::action("ui.toolbarPin", "Pin to Toolbar", "plus").with(json!({ "command": id }))
    });
    v.push(Item::action("ui.shortcutPin", if fav { "Unpin from Shortcuts" } else { "Pin to Shortcuts" }, "star").with(json!({ "command": id })));
    v.push(Item::sep());
    v.push(Item::action("ui.toolbarCustomize", "Customize Toolbar…", "settings"));
    v.push(Item::action("ui.toolbarReset", "Reset Toolbar", "restore").with(json!({ "tab": tab })));
    v
}

/// The toolbar's own menu (right-click on its empty space or a tab): every tab that can be shown
/// or hidden, ticked when on, then the button customisation of the selected tab.
pub fn toolbar_items(app: &SolveApp) -> Vec<Item> {
    let mut v = Vec::new();
    if !crate::render_workspace::active(app) {
        v.push(Item::heading(app.tr("prefs.toolbar_tabs")));
        for tab in crate::workspace::choosable_tabs() {
            let on = crate::workspace::tab_turned_on(app, tab);
            // The row reads as the tab does, in every language.
            let item = Item::action("ui.toolbarTab", &crate::toolbar::ribbon_label(app, tab), crate::workspace::tab_icon(tab))
                .with(json!({ "tab": tab, "shown": !on }));
            v.push(if on { item.key("current") } else { item });
        }
        // With every tab hidden there is no tab to customise.
        if !crate::workspace::visible_tabs(app).contains(&app.ui.tab.as_str()) {
            return v;
        }
        v.push(Item::sep());
    }
    v.push(Item::action("ui.toolbarCustomize", "Customize Toolbar…", "settings"));
    v.push(Item::action("ui.toolbarReset", "Reset Toolbar", "restore").with(json!({ "tab": app.ui.tab })));
    v
}

/// Run a toolbar item from a menu.
pub fn run(app: &mut SolveApp, action: &str, p: &Value) {
    let tab = p.get("tab").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| app.ui.tab.clone());
    let id = p.get("command").and_then(Value::as_str).unwrap_or("");
    match action {
        "ui.toolbarPin" => {
            if let Err(e) = pin(app, &tab, id) {
                app.set_status(e, true);
            }
        }
        "ui.toolbarRemove" => remove(app, &tab, id),
        "ui.toolbarReset" => reset(app, &tab),
        "ui.toolbarCustomize" => app.toolbar_custom.customizing = true,
        "ui.toolbarTab" => crate::workspace::set_tab_shown(app, &tab, p.get("shown").and_then(Value::as_bool).unwrap_or(true)),
        "ui.shortcutPin" => app.sbox.toggle_pin(id),
        _ => {}
    }
}

/// Shortcut keys, S box favourites, toolbar and navigation bar as one file (Keyboard Shortcuts › Export).
pub fn export(app: &SolveApp) -> Value {
    json!({ "format": "solidcraft-shortcuts/1", "shortcuts": app.keymap.prefs(), "favourites": app.sbox.pinned, "toolbar": app.toolbar_custom.prefs(), "navigation_bar": app.preferences.navigation_bar })
}

/// The largest shortcuts file Import… reads (hostile input; an export is a few tens of KiB).
pub const MAX_IMPORT_BYTES: usize = 1 << 20;

pub fn import(app: &mut SolveApp, v: &Value) -> Result<(), String> {
    // "solvecraft-shortcuts/1": files exported before the rename (compatibility shim).
    if !matches!(v.get("format").and_then(Value::as_str), Some("solidcraft-shortcuts/1" | "solvecraft-shortcuts/1")) {
        return Err("not a SolidCraft shortcuts file".into());
    }
    let navigation_bar = v
        .get("navigation_bar")
        .map(|value| {
            crate::navigation_bar::validate(value)?;
            serde_json::from_value::<crate::navigation_bar::NavigationBar>(value.clone()).map_err(|e| e.to_string())
        })
        .transpose()?;
    if let Some(bar) = navigation_bar {
        app.preferences.navigation_bar = bar;
    }
    if let Some(s) = v.get("shortcuts") {
        app.keymap.load(s);
    }
    if let Some(f) = v.get("favourites").and_then(Value::as_array) {
        app.sbox.pinned = f.iter().take(64).filter_map(|x| x.as_str().map(str::to_string)).collect();
    }
    if let Some(t) = v.get("toolbar") {
        app.toolbar_custom.load(t);
    }
    Ok(())
}

/// Customize Toolbar: every command of the tab's panels with a check box (on the toolbar or
/// not), how to drag them, and Reset Toolbar.
pub fn window(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.toolbar_custom.customizing {
        return;
    }
    let tab = app.ui.tab.clone();
    let mut changes: Vec<(&'static str, bool)> = Vec::new();
    let mut reset_tab = false;
    let t = crate::theme::Tokens::get();
    // This long card grows as sections open. A top-left pivot keeps its controls at their
    // published positions instead of shifting them after egui measures the taller content.
    let origin =
        egui::pos2((ctx.content_rect().center().x - t.form_card_width).max(ctx.content_rect().left()), ctx.content_rect().top() + t.space_24 * 2.0);
    let (closed, action) = ui_kit::Window::new(egui::Id::new("sc_toolbar_customize"), "settings", crate::i18n::tr_label("Customize Toolbar"))
        .placed(egui::Align2::LEFT_TOP, origin)
        .cards(2.0)
        .actions("", "Close", None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, &crate::i18n::format("toolbar.customize_note", &[("tab", app.tr_label(&tab))]), ui_kit::Text::Note("customize"));
            egui::ScrollArea::vertical()
                .max_height(
                    (ctx.content_rect().height() - crate::theme::Tokens::get().control_height * 8.0)
                        .min(crate::theme::Tokens::get().control_height * 20.0),
                )
                .show(ui, |ui| {
                    for (panel, cmds, n) in panel_lists(&tab) {
                        if cmds.is_empty() {
                            continue;
                        }
                        ui_kit::section(ui, panel, app.tr_label(panel), true, |ui| {
                            let shown = buttons(app, &tab, panel, &cmds, n);
                            for c in &cmds {
                                let mut on = shown.iter().any(|b| b.id == c.id) || pinned_on(app, &tab, c.id).is_some();
                                let r = ui_kit::checkbox(ui, app.tr_label(c.label), &mut on);
                                crate::scenario::publish_handle(&format!("customize:{}", c.id), r.rect.center());
                                if r.changed() {
                                    changes.push((c.id, on));
                                }
                            }
                        });
                    }
                });
            let reset = ui_kit::button(ui, crate::i18n::tr_label("Reset Toolbar"), ui_kit::Button::Normal);
            crate::scenario::publish_target("customize:reset", reset.rect);
            reset_tab |= reset.clicked();
        });
    for (id, on) in changes {
        if on {
            if let Err(e) = pin(app, &tab, id) {
                app.set_status(e, true);
            }
        } else {
            remove(app, &tab, id);
        }
    }
    if reset_tab {
        reset(app, &tab);
    }
    app.toolbar_custom.customizing = !closed && action != ui_kit::Action::Cancel;
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    /// Each tab's panel lists are worked out once and come back the same as worked out anew,
    /// for every tab the ribbon has (and an unknown one).
    #[test]
    fn panel_lists_are_kept_and_match_the_registry() {
        let tabs: std::collections::BTreeSet<&str> = crate::workspace::SECTIONS.iter().map(|s| s.tab).chain(["NO SUCH TAB"]).collect();
        let ids = |l: &PanelLists| l.iter().map(|(p, cmds, n)| (*p, cmds.iter().map(|c| c.id).collect::<Vec<_>>(), *n)).collect::<Vec<_>>();
        for tab in tabs {
            let fresh = panel_lists_of(tab);
            assert_eq!(ids(&panel_lists(tab)), ids(&fresh), "{tab}");
            assert_eq!(ids(&panel_lists(tab)), ids(&fresh), "{tab}, kept");
        }
    }

    #[test]
    fn legacy_pins_split_merge_and_preserve_removals_and_unknown_keys() {
        let mut custom = ToolbarCustom::default();
        custom.load(&json!({
            "DESIGN/SOLID/CREATE":["solid.hole","solid.revolve","solid.move"],
            "DESIGN/PLASTIC/CREATE":["plastic.boss","plastic.rest","sketch.create"],
            "DESIGN/SOLID/MODIFY":["solid.chamfer"],
            "DESIGN/UTILITIES/SELECT":[],
            "DESIGN/future/panel":["future.command"],
            "OTHER/SOLID/CREATE":["solid.extrude"]
        }));
        let create = &custom.panels["DESIGN/MODEL/CREATE"];
        assert_eq!(create, &["plastic.boss", "plastic.rest", "solid.hole", "solid.revolve"]);
        assert!(!create.iter().any(|id| id == "solid.extrude"));
        assert!(custom.panels["DESIGN/MODEL/ARRANGE"].contains(&"solid.move".into()));
        assert!(!custom.panels["DESIGN/MODEL/MODIFY"].contains(&"solid.fillet".into()));
        assert!(custom.panels["DESIGN/MODEL/MODIFY"].contains(&"solid.chamfer".into()));
        assert_eq!(custom.panels["DESIGN/future/panel"], vec!["future.command"]);
        assert!(custom.panels.contains_key("OTHER/SOLID/CREATE"));
        assert!(!custom.panels.contains_key("DESIGN/SOLID/CREATE"));
        let saved = custom.prefs();
        let mut reloaded = ToolbarCustom::default();
        reloaded.load(&saved);
        assert_eq!(custom, reloaded, "migration runs once, even for unchanged SKETCH/SURFACE keys");
    }

    #[test]
    fn empty_old_panels_keep_default_button_removals() {
        let mut custom = ToolbarCustom::default();
        custom.load(&json!({"DESIGN/SOLID/CREATE":[],"DESIGN/SKETCH/CONSTRAINTS":[],"DESIGN/PLASTIC/SETUP":[]}));
        assert!(!custom.panels["DESIGN/MODEL/SKETCH"].contains(&"sketch.create".into()));
        assert!(!custom.panels["DESIGN/MODEL/CREATE"].contains(&"solid.extrude".into()));
        assert!(!custom.panels["DESIGN/MANAGE/RULES"].contains(&"plastic.manage_rules".into()));
        assert_eq!(custom.panels["DESIGN/SKETCH/CONSTRAIN"], vec!["sketch.dimension"]);
    }
}

#[cfg(test)]
mod menu_order_tests {
    use super::*;

    /// A tab panel's drop-down ids as the menu shows them.
    fn menu(app: &SolveApp, tab: &str, panel: &str) -> Vec<&'static str> {
        let lists = panel_lists(tab);
        let cmds = &lists.iter().find(|(p, ..)| *p == panel).unwrap().1;
        order_grouped(app, tab, panel, crate::workspace::grouped_commands(tab, panel, cmds)).iter().map(|(_, c)| c.id).collect()
    }

    /// #5080: a command moves before another one of its menu; the order is saved with the
    /// toolbar (only once a menu changed), comes back on load and goes with Reset Toolbar.
    #[test]
    fn a_menu_command_moves_and_the_order_is_kept() {
        let mut h = crate::scenario::Harness::new();
        let before = menu(&h.app, "MODEL", "CREATE");
        assert!(!h.app.toolbar_custom.prefs().as_object().unwrap().contains_key("menus"), "nothing saved while no menu changed");
        move_in_menu(&mut h.app, "MODEL", "CREATE", "solid.sphere", Some("solid.extrude"));
        let after = menu(&h.app, "MODEL", "CREATE");
        assert_eq!(after.first(), Some(&"solid.sphere"));
        assert_eq!(after.len(), before.len());
        move_in_menu(&mut h.app, "MODEL", "CREATE", "solid.extrude", None);
        assert_eq!(menu(&h.app, "MODEL", "CREATE").last(), Some(&"solid.extrude"), "None: to the end");
        let mut reloaded = ToolbarCustom::default();
        reloaded.load(&h.app.toolbar_custom.prefs());
        assert_eq!(reloaded, h.app.toolbar_custom);
        reset(&mut h.app, "MODEL");
        assert_eq!(menu(&h.app, "MODEL", "CREATE"), before, "Reset Toolbar restores the order");
    }

    /// In a menu with group headings (MODEL MODIFY), a command stays in its group: a place in
    /// a later group puts it at its group's end, an earlier one at its start; headings stay on
    /// each group's first command.
    #[test]
    fn a_menu_command_stays_in_its_group() {
        let mut h = crate::scenario::Harness::new();
        move_in_menu(&mut h.app, "MODEL", "MODIFY", "solid.fillet", Some("solid.shell"));
        let ids = menu(&h.app, "MODEL", "MODIFY");
        assert_eq!(ids.get(..2), Some(&["solid.chamfer", "solid.fillet"][..]), "{ids:?}");
        move_in_menu(&mut h.app, "MODEL", "MODIFY", "solid.scale", Some("solid.chamfer"));
        let ids = menu(&h.app, "MODEL", "MODIFY");
        let shell = ids.iter().position(|id| *id == "solid.shell").unwrap();
        assert_eq!(ids.get(shell - 1), Some(&"solid.scale"), "to the start of its own group: {ids:?}");
        let lists = panel_lists("MODEL");
        let cmds = &lists.iter().find(|(p, ..)| *p == "MODIFY").unwrap().1;
        let grouped = order_grouped(&h.app, "MODEL", "MODIFY", crate::workspace::grouped_commands("MODEL", "MODIFY", cmds));
        let headings: Vec<_> = grouped.iter().filter_map(|(h, c)| h.map(|h| (h, c.id))).collect();
        assert_eq!(headings.first(), Some(&("Edges", "solid.chamfer")));
        assert!(headings.contains(&("Bodies", "solid.scale")), "{headings:?}");
    }

    /// A command the saved order doesn't name (added in a later version) keeps its built-in
    /// place; an unknown saved id is ignored.
    #[test]
    fn a_command_the_saved_order_lacks_keeps_its_place() {
        let mut h = crate::scenario::Harness::new();
        let built_in = menu(&h.app, "MODEL", "CREATE");
        let mut saved: Vec<String> = built_in.iter().filter(|id| **id != "solid.loft").rev().map(|id| id.to_string()).collect();
        saved.push("no.such.command".into());
        h.app.toolbar_custom.menus.insert(key("MODEL", "CREATE"), saved);
        let ids = menu(&h.app, "MODEL", "CREATE");
        assert_eq!(ids.iter().position(|id| *id == "solid.loft"), built_in.iter().position(|id| *id == "solid.loft"));
        assert_eq!(ids.len(), built_in.len());
    }
}
