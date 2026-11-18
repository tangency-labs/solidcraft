//! SolidCraft's macOS menu bar: its entries as kit menu items
//! (`solidcraft_ui_egui::window_chrome::native_menu`), each with a command id or a `ui.*` id, run
//! through the same actions as the application bar's File menu (`toolbar::FileAction`).
//!
//! Entries that are control actions use their ids and do the same there: `app.quit`,
//! `ui.prefs {open}` (Settings and Keyboard Shortcuts), `ui.agent_chat {toggle}`, `ui.help {item}`. Undo and Redo are the commands
//! `edit.undo` and `edit.redo`. The other entries' ids are defined here: `ui.file.new`,
//! `ui.file.open`, `ui.file.open_recent {path}`, `ui.file.clear_recent`, `ui.file.save`,
//! `ui.file.save_as`, `ui.file.save_json`, `ui.file.export {format}`,
//! `ui.file.export_mesh {format}`. Menus are `menu.file`, `menu.file.recent`, `menu.edit`,
//! `menu.window` and `menu.help`.
//!
//! The menu bar itself is macOS's; the entries and their ids build everywhere, so their tests run
//! on every system.

use serde_json::{Value, json};
use solidcraft_ui_egui::{
    SolveApp,
    toolbar::FileAction,
    ui_kit::Item,
    window_chrome::native_menu::{self, MenuBar},
};

/// The Open Recent submenu, refilled when the recent designs change.
const RECENT: &str = "menu.file.recent";
const OPEN_RECENT: &str = "ui.file.open_recent";
/// Recent designs listed.
#[cfg(target_os = "macos")]
const RECENT_COUNT: usize = 32;

/// File > Insert entries.
const INSERTS: [(&str, FileAction); 3] = [
    ("Insert STEP\u{2026}", FileAction::Command("file.insert_step")),
    ("Insert Mesh\u{2026}", FileAction::Command("file.insert_mesh")),
    ("Insert Design", FileAction::Command("component.insert_design")),
];

/// File > Export entries.
const EXPORTS: [(&str, FileAction); 7] = [
    ("Export STL\u{2026}", FileAction::ExportMesh("stl")),
    ("Export 3MF\u{2026}", FileAction::ExportMesh("3mf")),
    ("Export STEP\u{2026}", FileAction::Export("step")),
    ("Export Current View as DXF\u{2026}", FileAction::Command("file.export_view_dxf")),
    ("Export OBJ\u{2026}", FileAction::Export("obj")),
    ("Export URDF\u{2026}", FileAction::Export("urdf")),
    ("Export Design as JSON\u{2026}", FileAction::Export("json")),
];

const DESIGN_PROPERTIES: FileAction = FileAction::Command("document.metadata");

/// The other actions with a fixed id (all but Open Recent), to map an id back to its action.
const FIXED: &[FileAction] = &[
    FileAction::Settings,
    FileAction::Shortcuts,
    FileAction::Quit,
    FileAction::New,
    FileAction::Open,
    FileAction::ClearRecent,
    FileAction::Save,
    FileAction::SaveAs,
    FileAction::SaveJson,
    FileAction::VerifySignature,
    DESIGN_PROPERTIES,
    FileAction::Undo,
    FileAction::Redo,
    FileAction::AgentChat,
];

/// The Help menu's entries: the control channel's `ui.help {item}`.
const HELP_ID: &str = "ui.help";
/// The Help menu's items (`help::run`), top to bottom.
const HELP: [&str; 7] = ["docs", "updates", "shortcuts", "messages", "report", "crashes", "install_cli"];

fn help_label<'a>(app: &'a SolveApp, item: &str) -> &'a str {
    match item {
        "docs" => app.tr_label("Documentation"),
        "updates" => app.tr("update.check_for_updates"),
        "shortcuts" => app.tr_label("Keyboard Shortcuts"),
        "messages" => app.tr_label("Messages\u{2026}"),
        "report" => app.tr_label("Report an Issue\u{2026}"),
        "crashes" => app.tr_label("Crash Reports\u{2026}"),
        _ => app.tr_label("Install solidcraft-cli command"),
    }
}

/// What a menu entry runs.
#[derive(Debug, PartialEq)]
enum Entry {
    File(FileAction),
    Help(&'static str),
}

/// The entry's id and parameters for `action`.
fn id_of(action: &FileAction) -> (String, Option<Value>) {
    let fixed = |id: &str| (id.to_string(), None);
    match action {
        FileAction::Settings => ("ui.prefs".into(), Some(json!({ "open": "General" }))),
        FileAction::Shortcuts => ("ui.prefs".into(), Some(json!({ "open": "Shortcuts" }))),
        FileAction::Quit => fixed("app.quit"),
        FileAction::New => fixed("ui.file.new"),
        FileAction::Open => fixed("ui.file.open"),
        FileAction::OpenRecent(path) => (OPEN_RECENT.into(), Some(json!({ "path": path }))),
        FileAction::ClearRecent => fixed("ui.file.clear_recent"),
        FileAction::Save => fixed("ui.file.save"),
        FileAction::SaveAs => fixed("ui.file.save_as"),
        FileAction::SaveJson => fixed("ui.file.save_json"),
        FileAction::VerifySignature => fixed("file.verify"),
        FileAction::Export(format) => ("ui.file.export".into(), Some(json!({ "format": format }))),
        FileAction::ExportMesh(format) => ("ui.file.export_mesh".into(), Some(json!({ "format": format }))),
        FileAction::Command(command) => fixed(command),
        FileAction::Undo => fixed("edit.undo"),
        FileAction::Redo => fixed("edit.redo"),
        FileAction::AgentChat => ("ui.agent_chat".into(), Some(json!({ "toggle": true }))),
    }
}

/// The entry with `id` and `params`, `None` for one this menu doesn't have.
fn entry_of(id: &str, params: Option<&Value>) -> Option<Entry> {
    if id == OPEN_RECENT {
        return params.and_then(|p| p.get("path")).and_then(Value::as_str).map(|path| Entry::File(FileAction::OpenRecent(path.to_string())));
    }
    if id == HELP_ID {
        let item = params.and_then(|p| p.get("item")).and_then(Value::as_str);
        return HELP.into_iter().find(|h| Some(*h) == item).map(Entry::Help);
    }
    let wanted = (id.to_string(), params.cloned());
    FIXED.iter().cloned().chain(INSERTS.map(|(_, a)| a)).chain(EXPORTS.map(|(_, a)| a)).find(|a| id_of(a) == wanted).map(Entry::File)
}

/// The entries with a key, as menu accelerators.
const SHORTCUTS: [(FileAction, &str); 8] = [
    (FileAction::Settings, "Cmd+,"),
    (FileAction::Quit, "Cmd+Q"),
    (FileAction::New, "Cmd+N"),
    (FileAction::Open, "Cmd+O"),
    (FileAction::Save, "Cmd+S"),
    (FileAction::SaveAs, "Cmd+Shift+S"),
    (FileAction::Undo, "Cmd+Z"),
    (FileAction::Redo, "Cmd+Shift+Z"),
];

fn item(label: &str, action: &FileAction) -> Item {
    let (id, params) = id_of(action);
    let shortcut = SHORTCUTS.iter().find(|(a, _)| a == action).map_or("", |(_, k)| *k);
    let mut item = Item::action(&id, label, "").key(shortcut);
    item.params = params;
    item
}

fn submenu(id: &str, label: &str, children: Vec<Item>) -> Item {
    let mut menu = Item::action(id, label, "");
    menu.children = children;
    menu
}

fn recent_items(app: &SolveApp, paths: &[String]) -> Vec<Item> {
    let mut items: Vec<Item> = paths.iter().map(|path| item(path, &FileAction::OpenRecent(path.clone()))).collect();
    items.push(Item::sep());
    items.push(item(app.tr("ui.clear_recent"), &FileAction::ClearRecent));
    items
}

fn menu_bar(app: &SolveApp, recent: &[String]) -> MenuBar {
    let more = |key: &str| format!("{}\u{2026}", app.tr(key));
    let mut file = vec![
        item(app.tr("ui.new_design"), &FileAction::New),
        item(&more("ui.open"), &FileAction::Open),
        submenu(RECENT, app.tr("ui.open_recent"), recent_items(app, recent)),
        Item::sep(),
    ];
    file.extend(INSERTS.iter().map(|(label, action)| item(app.tr_label(label), action)));
    file.extend([
        Item::sep(),
        item(app.tr("ui.save"), &FileAction::Save),
        item(&more("ui.save_as"), &FileAction::SaveAs),
        item(app.tr_label("Save As JSON\u{2026}"), &FileAction::SaveJson),
        item(app.tr("cmd.file.verify"), &FileAction::VerifySignature),
        Item::sep(),
    ]);
    file.extend(EXPORTS.iter().map(|(label, action)| item(app.tr_label(label), action)));
    file.extend([Item::sep(), item(app.tr_label("Design Properties"), &DESIGN_PROPERTIES)]);
    let edit = vec![item(app.tr("ui.undo"), &FileAction::Undo), item(app.tr("ui.redo"), &FileAction::Redo)];
    let window = vec![item(app.tr_label("Agent Chat"), &FileAction::AgentChat)];
    let help = HELP.iter().map(|h| Item::action(HELP_ID, help_label(app, h), "").with(json!({ "item": h }))).collect();
    MenuBar {
        title: "SolidCraft".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        app: vec![item(&more("ui.preferences"), &FileAction::Settings), item(&more("ui.keyboard_shortcuts"), &FileAction::Shortcuts)],
        quit: item(&format!("{} SolidCraft", app.tr("ui.quit")), &FileAction::Quit),
        menus: vec![
            submenu("menu.file", app.tr("ui.file"), file),
            submenu("menu.edit", app.tr_label("Edit"), edit),
            submenu(native_menu::WINDOW, "Window", window),
            submenu("menu.help", app.tr("ui.help"), help),
        ],
    }
}

/// The menu bar, owned and updated on eframe's main thread.
#[cfg(target_os = "macos")]
pub struct NativeMenu {
    menu: native_menu::NativeMenu,
    recent_paths: Vec<String>,
}

#[cfg(target_os = "macos")]
impl NativeMenu {
    pub fn new(app: &SolveApp, ctx: &egui::Context) -> Result<Self, String> {
        let recent_paths: Vec<String> = app.home.recent.iter().take(RECENT_COUNT).cloned().collect();
        let menu = native_menu::NativeMenu::new(&menu_bar(app, &recent_paths), ctx)?;
        let native = Self { menu, recent_paths };
        native.update(app, ctx);
        Ok(native)
    }

    pub fn raw_input_hook(&mut self, app: &mut SolveApp, ctx: &egui::Context, raw: &mut egui::RawInput) {
        for (id, params) in self.menu.events() {
            match entry_of(&id, params.as_ref()) {
                Some(Entry::Help(item)) => solidcraft_ui_egui::help::run(app, ctx, item),
                // A focused text field takes its own Undo and Redo.
                Some(Entry::File(action @ (FileAction::Undo | FileAction::Redo))) if ctx.egui_wants_keyboard_input() => {
                    raw.events.push(egui::Event::Key {
                        key: egui::Key::Z,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers { command: true, mac_cmd: true, shift: matches!(action, FileAction::Redo), ..Default::default() },
                    });
                }
                Some(Entry::File(action)) => action.run(app),
                None => log::warn!("native menu: no entry `{id}`"),
            }
        }
    }

    fn update(&self, app: &SolveApp, ctx: &egui::Context) {
        let focus = ctx.egui_wants_keyboard_input();
        self.menu.update(|id, params| match entry_of(id, params) {
            Some(Entry::File(action)) => action.menu_enabled(app, focus),
            Some(Entry::Help(item)) => solidcraft_ui_egui::help::available(app, item),
            None => false,
        });
    }

    pub fn tick(&mut self, app: &mut SolveApp, ctx: &egui::Context) {
        if !self.recent_paths.iter().eq(app.home.recent.iter().take(RECENT_COUNT)) {
            self.recent_paths = app.home.recent.iter().take(RECENT_COUNT).cloned().collect();
            if let Err(error) = self.menu.replace(RECENT, &recent_items(app, &self.recent_paths)) {
                app.set_status(format!("Recent designs menu: {error}"), true);
            }
        }
        self.update(app, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every action the menu offers, Open Recent with a path.
    fn actions() -> Vec<FileAction> {
        let mut all: Vec<FileAction> = FIXED.iter().cloned().chain(INSERTS.map(|(_, a)| a)).chain(EXPORTS.map(|(_, a)| a)).collect();
        all.push(FileAction::OpenRecent("/work/a.solidcraft".into()));
        all
    }

    #[test]
    fn every_action_and_help_item_round_trips_through_its_id() {
        let mut seen = vec![];
        for action in actions() {
            let (id, params) = id_of(&action);
            assert!(!seen.contains(&(id.clone(), params.clone())), "two actions share `{id}` {params:?}");
            assert_eq!(entry_of(&id, params.as_ref()), Some(Entry::File(action)), "`{id}` {params:?}");
            seen.push((id, params));
        }
        for h in HELP {
            assert_eq!(entry_of(HELP_ID, Some(&json!({ "item": h }))), Some(Entry::Help(h)));
        }
        // The control channel's ids where it has one.
        assert_eq!(id_of(&FileAction::Quit), ("app.quit".to_string(), None));
        assert_eq!(id_of(&FileAction::Settings), ("ui.prefs".to_string(), Some(json!({"open": "General"}))));
        assert_eq!(id_of(&FileAction::Shortcuts), ("ui.prefs".to_string(), Some(json!({"open": "Shortcuts"}))));
        assert_eq!(id_of(&FileAction::AgentChat), ("ui.agent_chat".to_string(), Some(json!({"toggle": true}))));
        assert_eq!(entry_of("ui.file.export", Some(&json!({"format": "step"}))), Some(Entry::File(FileAction::Export("step"))));
        for (id, params) in [
            ("ui.file.export", None),
            ("ui.file.export", Some(json!({"format": "dwg"}))),
            (HELP_ID, Some(json!({"item": "nothing"}))),
            (HELP_ID, None),
            ("ui.prefs", None),
            ("solid.nothing", None),
            (OPEN_RECENT, None),
            (RECENT, None),
            (native_menu::WINDOW, None),
        ] {
            assert_eq!(entry_of(id, params.as_ref()), None, "{id} {params:?}");
        }
    }

    #[test]
    fn every_shortcut_is_one_actions_and_the_menu_shows_it() {
        for (i, (action, key)) in SHORTCUTS.iter().enumerate() {
            assert!(SHORTCUTS.iter().skip(i + 1).all(|(a, k)| a != action && k != key), "{action:?} {key}");
            assert_eq!(item("label", action).shortcut, *key);
        }
        assert_eq!(item("label", &FileAction::SaveJson).shortcut, "");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn menu_shortcuts_parse_as_the_keys_they_were() {
        use muda::accelerator::{Accelerator, Code, Modifiers};
        let (cmd, shift) = (Modifiers::META, Modifiers::META | Modifiers::SHIFT);
        let want = [
            Accelerator::new(cmd, Code::Comma),
            Accelerator::new(cmd, Code::KeyQ),
            Accelerator::new(cmd, Code::KeyN),
            Accelerator::new(cmd, Code::KeyO),
            Accelerator::new(cmd, Code::KeyS),
            Accelerator::new(shift, Code::KeyS),
            Accelerator::new(cmd, Code::KeyZ),
            Accelerator::new(shift, Code::KeyZ),
        ];
        for ((action, key), want) in SHORTCUTS.iter().zip(want) {
            assert_eq!(key.parse::<Accelerator>().ok(), Some(want), "{action:?}");
        }
    }

    #[test]
    fn every_entry_of_the_menu_bar_runs_its_action() {
        fn leaves(items: &[Item], out: &mut Vec<Item>) {
            for item in items {
                if item.children.is_empty() {
                    out.push(item.clone());
                } else {
                    leaves(&item.children, out);
                }
            }
        }
        let app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let bar = menu_bar(&app, &["/work/a.solidcraft".to_string()]);
        let mut entries = vec![];
        leaves(&bar.app, &mut entries);
        leaves(std::slice::from_ref(&bar.quit), &mut entries);
        leaves(&bar.menus, &mut entries);
        entries.retain(|i| i.id != "-");
        // App menu 2 + Quit; File: New, Open, 1 recent, Clear, 3 inserts, Save, Save As, JSON,
        // Verify Signature, 7 exports, Properties; Edit 2; Window 1; Help 7.
        assert_eq!(entries.len(), 3 + 4 + 3 + 4 + 7 + 1 + 2 + 1 + 7, "{:?}", entries.iter().map(|i| &i.id).collect::<Vec<_>>());
        for item in &entries {
            assert!(entry_of(&item.id, item.params.as_ref()).is_some(), "`{}` {:?} runs nothing", item.id, item.params);
        }
        assert!(bar.menus.iter().all(|m| m.id.starts_with("menu.")), "menu ids are never actions");
    }

    /// The entries whose ids are control actions do over the control channel what they do in
    /// the menu.
    #[test]
    fn shared_ids_mean_the_same_over_the_control_channel() {
        use solidcraft_ui_egui::scenario::Harness;
        let mut h = Harness::new();
        h.app.preferences.ai_features = true;
        h.app.agent_chat.open = false;
        let (id, params) = id_of(&FileAction::AgentChat);
        for open in [true, false] {
            let out = h.call(&id, params.clone().unwrap_or_default());
            assert_eq!(out["ok"], true, "{out}");
            assert_eq!(h.app.agent_chat.open, open, "Agent Chat toggles as in the menu");
        }
        for action in [FileAction::Settings, FileAction::Shortcuts] {
            let (id, params) = id_of(&action);
            let out = h.call(&id, params.unwrap_or_default());
            assert_eq!(out["ok"], true, "{action:?}: {out}");
        }
        // install_cli links into the system; its menu and control paths are the same `help::run`.
        for item in HELP.into_iter().filter(|h| *h != "install_cli") {
            let out = h.call(HELP_ID, json!({ "item": item }));
            assert_eq!(out["ok"], true, "{item}: {out}");
        }
        assert_eq!(h.call(HELP_ID, json!({"item": "nothing"}))["ok"], false, "an unknown help item is an error");
        let out = h.call(&id_of(&FileAction::Quit).0, json!({}));
        assert_eq!((out["ok"].clone(), h.app.quit_requested), (json!(true), true), "{out}");
    }
}
