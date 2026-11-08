//! SolidCraft's macOS menu bar: its entries as kit menu items
//! (`solidcraft_ui_egui::window_chrome::native_menu`), each with a command id or a `ui.*` id, run
//! through the same actions as the application bar's File menu (`toolbar::FileAction`).
//!
//! The `ui.*` ids of entries that are not commands, defined here:
//! `ui.app.preferences`, `ui.app.shortcuts`, `ui.app.quit`, `ui.file.new`, `ui.file.open`,
//! `ui.file.open_recent {path}`, `ui.file.clear_recent`, `ui.file.save`, `ui.file.save_as`,
//! `ui.file.save_json`, `ui.file.export {format}`, `ui.file.export_mesh {format}`,
//! `ui.window.agent_chat`, `ui.help.<item>` (the Help menu's items, `help::run`). Undo and Redo
//! are the commands `edit.undo` and `edit.redo`.

use serde_json::{Value, json};
use solidcraft_ui_egui::{
    SolveApp,
    toolbar::FileAction,
    ui_kit::Item,
    window_chrome::native_menu::{self, MenuBar},
};

/// The Open Recent submenu, refilled when the recent designs change.
const RECENT: &str = "ui.file.recent_menu";
const OPEN_RECENT: &str = "ui.file.open_recent";
/// Recent designs listed.
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
const FIXED: [FileAction; 13] = [
    FileAction::Settings,
    FileAction::Shortcuts,
    FileAction::Quit,
    FileAction::New,
    FileAction::Open,
    FileAction::ClearRecent,
    FileAction::Save,
    FileAction::SaveAs,
    FileAction::SaveJson,
    DESIGN_PROPERTIES,
    FileAction::Undo,
    FileAction::Redo,
    FileAction::AgentChat,
];

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
        FileAction::Settings => fixed("ui.app.preferences"),
        FileAction::Shortcuts => fixed("ui.app.shortcuts"),
        FileAction::Quit => fixed("ui.app.quit"),
        FileAction::New => fixed("ui.file.new"),
        FileAction::Open => fixed("ui.file.open"),
        FileAction::OpenRecent(path) => (OPEN_RECENT.into(), Some(json!({ "path": path }))),
        FileAction::ClearRecent => fixed("ui.file.clear_recent"),
        FileAction::Save => fixed("ui.file.save"),
        FileAction::SaveAs => fixed("ui.file.save_as"),
        FileAction::SaveJson => fixed("ui.file.save_json"),
        FileAction::Export(format) => ("ui.file.export".into(), Some(json!({ "format": format }))),
        FileAction::ExportMesh(format) => ("ui.file.export_mesh".into(), Some(json!({ "format": format }))),
        FileAction::Command(command) => fixed(command),
        FileAction::Undo => fixed("edit.undo"),
        FileAction::Redo => fixed("edit.redo"),
        FileAction::AgentChat => fixed("ui.window.agent_chat"),
    }
}

/// The entry with `id` and `params`, `None` for one this menu doesn't have.
fn entry_of(id: &str, params: Option<&Value>) -> Option<Entry> {
    if id == OPEN_RECENT {
        return params.and_then(|p| p.get("path")).and_then(Value::as_str).map(|path| Entry::File(FileAction::OpenRecent(path.to_string())));
    }
    if let Some(item) = id.strip_prefix("ui.help.") {
        return HELP.into_iter().find(|h| *h == item).map(Entry::Help);
    }
    let wanted = (id.to_string(), params.cloned());
    FIXED.into_iter().chain(INSERTS.map(|(_, a)| a)).chain(EXPORTS.map(|(_, a)| a)).find(|a| id_of(a) == wanted).map(Entry::File)
}

fn item(label: &str, action: &FileAction, shortcut: &str) -> Item {
    let (id, params) = id_of(action);
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
    let mut items: Vec<Item> = paths.iter().map(|path| item(path, &FileAction::OpenRecent(path.clone()), "")).collect();
    items.push(Item::sep());
    items.push(item(app.tr("ui.clear_recent"), &FileAction::ClearRecent, ""));
    items
}

fn menu_bar(app: &SolveApp, recent: &[String]) -> MenuBar {
    let more = |key: &str| format!("{}\u{2026}", app.tr(key));
    let mut file = vec![
        item(app.tr("ui.new_design"), &FileAction::New, "Cmd+N"),
        item(&more("ui.open"), &FileAction::Open, "Cmd+O"),
        submenu(RECENT, app.tr("ui.open_recent"), recent_items(app, recent)),
        Item::sep(),
    ];
    file.extend(INSERTS.iter().map(|(label, action)| item(app.tr_label(label), action, "")));
    file.extend([
        Item::sep(),
        item(app.tr("ui.save"), &FileAction::Save, "Cmd+S"),
        item(&more("ui.save_as"), &FileAction::SaveAs, "Cmd+Shift+S"),
        item(app.tr_label("Save As JSON\u{2026}"), &FileAction::SaveJson, ""),
        Item::sep(),
    ]);
    file.extend(EXPORTS.iter().map(|(label, action)| item(app.tr_label(label), action, "")));
    file.extend([Item::sep(), item(app.tr_label("Design Properties"), &DESIGN_PROPERTIES, "")]);
    let edit = vec![item(app.tr("ui.undo"), &FileAction::Undo, "Cmd+Z"), item(app.tr("ui.redo"), &FileAction::Redo, "Cmd+Shift+Z")];
    let window = vec![item(app.tr_label("Agent Chat"), &FileAction::AgentChat, "")];
    let help = HELP.iter().map(|h| Item::action(&format!("ui.help.{h}"), help_label(app, h), "")).collect();
    MenuBar {
        title: "SolidCraft".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        app: vec![item(&more("ui.preferences"), &FileAction::Settings, "Cmd+,"), item(&more("ui.keyboard_shortcuts"), &FileAction::Shortcuts, "")],
        quit: item(&format!("{} SolidCraft", app.tr("ui.quit")), &FileAction::Quit, "Cmd+Q"),
        menus: vec![
            submenu("ui.file", app.tr("ui.file"), file),
            submenu("ui.edit", app.tr_label("Edit"), edit),
            submenu(native_menu::WINDOW, "Window", window),
            submenu("ui.help", app.tr("ui.help"), help),
        ],
    }
}

/// The menu bar, owned and updated on eframe's main thread.
pub struct NativeMenu {
    menu: native_menu::NativeMenu,
    recent_paths: Vec<String>,
}

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

    fn leaves(items: &[Item], out: &mut Vec<Item>) {
        for item in items {
            if item.children.is_empty() {
                out.push(item.clone());
            } else {
                leaves(&item.children, out);
            }
        }
    }

    #[test]
    fn every_menu_entry_maps_back_to_its_action() {
        let app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let bar = menu_bar(&app, &["/work/a.solidcraft".to_string()]);
        let mut entries = vec![];
        leaves(&bar.app, &mut entries);
        leaves(std::slice::from_ref(&bar.quit), &mut entries);
        leaves(&bar.menus, &mut entries);
        entries.retain(|i| i.id != "-");
        // App menu 2 + Quit; File: New, Open, 1 recent, Clear, 3 inserts, Save, Save As, JSON,
        // 7 exports, Properties; Edit 2; Window 1; Help 7.
        assert_eq!(entries.len(), 3 + 4 + 3 + 3 + 7 + 1 + 2 + 1 + 7, "{:?}", entries.iter().map(|i| &i.id).collect::<Vec<_>>());
        for item in &entries {
            let entry = entry_of(&item.id, item.params.as_ref());
            assert!(entry.is_some(), "`{}` {:?} runs nothing", item.id, item.params);
            if let Some(Entry::File(action)) = entry {
                assert_eq!(id_of(&action), (item.id.clone(), item.params.clone()));
            }
        }
        assert_eq!(
            entry_of(OPEN_RECENT, Some(&json!({"path": "/work/a.solidcraft"}))),
            Some(Entry::File(FileAction::OpenRecent("/work/a.solidcraft".into())))
        );
        assert_eq!(entry_of("ui.file.export", Some(&json!({"format": "step"}))), Some(Entry::File(FileAction::Export("step"))));
        assert_eq!(entry_of("ui.help.docs", None), Some(Entry::Help("docs")));
        for unknown in ["ui.file.export", "ui.help.nothing", "solid.nothing", OPEN_RECENT] {
            assert_eq!(entry_of(unknown, None), None, "{unknown}");
        }
    }

    #[test]
    fn menu_shortcuts_keep_their_keys() {
        use muda::accelerator::{Accelerator, Code, Modifiers};
        let app = SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        let bar = menu_bar(&app, &[]);
        let mut entries = vec![];
        leaves(&bar.app, &mut entries);
        leaves(std::slice::from_ref(&bar.quit), &mut entries);
        leaves(&bar.menus, &mut entries);
        let key = |id: &str| entries.iter().find(|i| i.id == id).and_then(|i| i.shortcut.parse::<Accelerator>().ok());
        let (cmd, shift) = (Modifiers::META, Modifiers::META | Modifiers::SHIFT);
        for (id, want) in [
            ("ui.app.preferences", Accelerator::new(cmd, Code::Comma)),
            ("ui.app.quit", Accelerator::new(cmd, Code::KeyQ)),
            ("ui.file.new", Accelerator::new(cmd, Code::KeyN)),
            ("ui.file.open", Accelerator::new(cmd, Code::KeyO)),
            ("ui.file.save", Accelerator::new(cmd, Code::KeyS)),
            ("ui.file.save_as", Accelerator::new(shift, Code::KeyS)),
            ("edit.undo", Accelerator::new(cmd, Code::KeyZ)),
            ("edit.redo", Accelerator::new(shift, Code::KeyZ)),
        ] {
            assert_eq!(key(id), Some(want), "{id}");
        }
    }
}
