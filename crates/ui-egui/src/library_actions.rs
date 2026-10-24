//! Thumbnail menu actions share the engine commands used by scripts and other frontends.
use super::{Entry, open_entry};
use crate::SolveApp;
use serde_json::json;

#[derive(Clone, Copy, PartialEq)]
enum Action {
    Open,
    Insert,
    Rename,
    Move,
    Copy,
    Trash,
    Reveal,
}

pub(super) struct Dialog {
    entry: Entry,
    action: Action,
    name: String,
    focus: bool,
    error: Option<String>,
}

fn label(action: Action) -> &'static str {
    match action {
        Action::Open => "Open",
        Action::Insert => "Insert into Current Design",
        Action::Rename => "Rename",
        Action::Move => "Move",
        Action::Copy => "Copy",
        Action::Trash => "Move to Trash",
        Action::Reveal => "Show in Folder",
    }
}

pub(super) fn context_menu(app: &mut SolveApp, ui: &egui::Ui, response: &egui::Response, entry: &Entry) {
    let keyboard = response.has_focus() && ui.input(|i| i.modifiers.shift && i.key_pressed(egui::Key::F10));
    if keyboard {
        egui::Popup::open_id(ui.ctx(), crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Popup).context_menu(response).get_id());
    }
    crate::ui_kit::Container::new(ui.ctx(), crate::ui_kit::Surface::Popup).context_menu(response).show(|ui| {
        for action in [Action::Open, Action::Insert, Action::Rename, Action::Move, Action::Copy, Action::Trash, Action::Reveal] {
            if entry.folder && action != Action::Open {
                continue;
            }
            if matches!(action, Action::Rename | Action::Reveal) {
                ui.separator();
            }
            let path = entry.path.to_str();
            let enabled = path.is_some()
                && !app.file_picker_busy()
                && !app.file_load.busy()
                && (action != Action::Reveal || app.services.show_in_folder.is_some());
            if crate::ui_kit::button(ui, label(action), crate::ui_kit::Button::Enabled(enabled)).clicked() {
                ui.close();
                perform(app, entry.clone(), action);
            }
        }
    });
}

fn perform(app: &mut SolveApp, entry: Entry, action: Action) {
    let Some(path) = entry.path.to_str() else {
        app.set_status("This filename is not valid UTF-8.", true);
        return;
    };
    match action {
        Action::Open => open_entry(app, entry),
        Action::Insert => {
            if app.run("component.insert_design", json!({"path":path})).is_ok() {
                app.home.open = false;
            }
        }
        Action::Reveal => {
            if let Some(show) = &app.services.show_in_folder {
                show(path);
            }
        }
        Action::Rename | Action::Trash => {
            app.library.action_dialog = Some(Dialog { name: entry.name.clone(), entry, action, focus: true, error: None });
        }
        Action::Copy => {
            let stem = entry.path.file_stem().and_then(|s| s.to_str()).unwrap_or("Design");
            let extension = entry.path.extension().and_then(|s| s.to_str()).unwrap_or("solidcraft").to_string();
            let suggested = format!("{stem} copy.{extension}");
            app.pick_save(&suggested, &[&extension], move |app, destination| {
                let Some(destination) = destination else { return };
                execute(app, &entry, action, Some(std::path::Path::new(&destination)));
            });
        }
        Action::Move => {
            app.pick_folder(move |app, folder| {
                let Some(folder) = folder else { return };
                let destination = std::path::Path::new(&folder).join(&entry.name);
                execute(app, &entry, action, Some(&destination));
            });
        }
    }
}

fn execute(app: &mut SolveApp, entry: &Entry, action: Action, destination: Option<&std::path::Path>) -> bool {
    let Some(path) = entry.path.to_str() else {
        return false;
    };
    let command = match action {
        Action::Copy => "file.copy",
        Action::Trash => "file.trash",
        _ => "file.move",
    };
    let result = app.run(command, json!({"path":path, "destination":destination}));
    match result {
        Ok(_) => {
            // The Home recent list must not point at a path removed through this panel.
            if action != Action::Copy {
                let new = destination.and_then(|p| p.to_str());
                for recent in &mut app.home.recent {
                    if recent == path {
                        *recent = new.unwrap_or("").to_string();
                    }
                }
                app.home.recent.retain(|p| !p.is_empty());
            }
            app.set_status(format!("{} completed.", label(action)), false);
            true
        }
        Err(error) => {
            app.library.error = Some(error);
            false
        }
    }
}

pub(super) fn dialog(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.library.action_dialog.take() else { return };
    let mut confirm = false;
    let mut cancel = false;
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::new("library_file_action"), "folder", label(dialog.action))
        .actions(label(dialog.action), "Cancel", None)
        .fields()
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, true, |ui| {
            ui.label(&dialog.entry.name);
            if dialog.action == Action::Rename {
                let input = crate::ui_kit::bounded_cell_editor(
                    ui,
                    ui.id().with("library_filename"),
                    &mut dialog.name,
                    "library.new_filename",
                    dialog.error.is_some(),
                    255,
                );
                input.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, crate::i18n::tr("library.new_filename")));
                if dialog.focus {
                    input.request_focus();
                    dialog.focus = false;
                }
                confirm |= input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            } else {
                ui.label("This file will go to the system Trash, where you can restore it.");
            }
            if let Some(error) = &dialog.error {
                ui.label(error);
            }
        });
    confirm |= action == crate::ui_kit::Action::Ok;
    cancel |= closed || action == crate::ui_kit::Action::Cancel;
    cancel |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if cancel {
        return;
    }
    if confirm {
        let destination = if dialog.action == Action::Rename {
            let name = dialog.name.trim();
            if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
                dialog.error = Some("Enter a filename without directory separators.".into());
                app.library.action_dialog = Some(dialog);
                return;
            }
            Some(dialog.entry.path.with_file_name(name))
        } else {
            None
        };
        if execute(app, &dialog.entry, dialog.action, destination.as_deref()) {
            return;
        }
        dialog.error = app.library.error.clone();
    }
    app.library.action_dialog = Some(dialog);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    fn entry(h: &Harness) -> Entry {
        let path = h.dir.join("MenuTest.solidcraft");
        std::fs::write(&path, solidcraft_engine::io::write_design(&solidcraft_engine::doc::Document::new("Menu Test"))).unwrap();
        Entry { path, name: "MenuTest.solidcraft".into(), folder: false }
    }

    #[test]
    fn thumbnail_context_menu_has_supported_actions_and_escape_closes_it() {
        for mode in [crate::shell::LayoutMode::Floating, crate::shell::LayoutMode::Docked] {
            let mut h = Harness::new();
            h.call("ui.set", json!({"layout":mode}));
            h.app.services.pick_folder = Some(Box::new(|| Box::pin(std::future::ready(None))));
            let e = entry(&h);
            h.app.library.set_root(h.dir.clone());
            for _ in 0..200 {
                h.app.library.poll(&h.ctx);
                if !h.app.library.loading() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            h.frames(3);
            h.step(&json!({"click":{"handle":"library:entry:0"},"button":"right"})).unwrap();
            h.frames(2);
            for label in ["Open", "Insert into Current Design", "Rename", "Move", "Copy", "Move to Trash"] {
                assert!(h.widget(label).is_some(), "{label}");
            }
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.widget("Insert into Current Design").is_none());
            assert!(e.path.exists());
            h.app.home.open = false;
            h.frames(2);
            h.step(&json!({"click":{"handle":"library:entry:0"},"button":"right"})).unwrap();
            h.frames(2);
            h.step(&json!({"widget":"Rename"})).unwrap();
            h.frames(2);
            assert!(h.app.library.action_dialog.is_some());
            assert!(!h.app.home.open);
            assert!(!h.app.file_load.busy());
            assert!(h.app.session.path.is_none());
            h.step(&json!({"key":"Escape"})).unwrap();
            h.frames(2);
            assert!(h.app.library.action_dialog.is_none());
            assert!(e.path.exists());
        }
    }

    #[test]
    fn canceled_pickers_and_rename_keep_saved_file_and_unsaved_design() {
        let mut h = Harness::new();
        let e = entry(&h);
        h.app.services.pick_folder = Some(Box::new(|| Box::pin(std::future::ready(None))));
        h.app.run("solid.box", json!({"length":10,"width":12,"height":14})).unwrap();
        let before = h.app.session.doc.to_json();
        perform(&mut h.app, e.clone(), Action::Move);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        assert!(e.path.exists());
        perform(&mut h.app, e.clone(), Action::Rename);
        h.frames(2);
        h.step(&json!({"key":"Escape"})).unwrap();
        h.frames(2);
        assert!(h.app.library.action_dialog.is_none());
        assert!(e.path.exists());
        assert_eq!(h.app.session.doc.to_json(), before);
    }

    #[test]
    fn file_in_inactive_tab_is_protected_from_move_and_trash() {
        let mut h = Harness::new();
        let e = entry(&h);
        h.app.run("file.save", json!({"path":e.path})).unwrap();
        crate::documents::new_design(&mut h.app);
        h.app.library.set_root(h.dir.clone());
        for _ in 0..200 {
            h.app.library.poll(&h.ctx);
            if !h.app.library.loading() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!h.app.library.loading(), "initial folder scan must finish before checking rejected operations");
        assert!(crate::documents::uses_file(&h.app, e.path.to_str().unwrap()));
        assert!(!execute(&mut h.app, &e, Action::Trash, None));
        assert!(h.app.run("file.trash", json!({"path":e.path})).is_err());
        assert!(h.app.run("file.move", json!({"path":e.path,"destination":h.dir.join("Moved.solidcraft")})).is_err());
        assert!(e.path.exists());
        assert!(!h.app.library.loading(), "rejected open-file removals must not start a scan");
    }
    #[test]
    fn copy_picker_writes_a_separate_design_and_cancellation_writes_nothing() {
        let mut h = Harness::new();
        let e = entry(&h);
        let destination = h.dir.join("Copy.solidcraft");
        let target = destination.to_string_lossy().into_owned();
        h.app.services.pick_save = Some(Box::new(move |_, _| {
            let target = target.clone();
            Box::pin(std::future::ready(Some(target)))
        }));
        perform(&mut h.app, e.clone(), Action::Copy);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        assert_eq!(std::fs::read(&destination).unwrap(), std::fs::read(&e.path).unwrap());
        h.app.services.pick_save = Some(Box::new(|_, _| Box::pin(std::future::ready(None))));
        perform(&mut h.app, e.clone(), Action::Copy);
        crate::file_picker::tick(&mut h.app, &h.ctx);
        assert!(e.path.exists());
    }
    #[test]
    fn insert_uses_existing_link_command_and_can_be_undone() {
        let mut h = Harness::new();
        let e = entry(&h);
        let mut part = solidcraft_engine::Session::default();
        part.execute("solid.box", &json!({"length":10,"width":12,"height":14})).unwrap();
        part.execute("file.save", &json!({"path":e.path})).unwrap();
        let original = std::fs::read(&e.path).unwrap();
        let before = h.app.session.doc.to_json();
        perform(&mut h.app, e.clone(), Action::Insert);
        assert!(!h.app.session.doc.links.is_empty());
        assert!(!h.app.home.open);
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!(std::fs::read(&e.path).unwrap(), original);
    }

    #[test]
    fn reveal_uses_the_host_file_manager_without_modifying_the_design() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let mut h = Harness::new();
        let e = entry(&h);
        let shown = Rc::new(RefCell::new(String::new()));
        let answer = shown.clone();
        h.app.services.show_in_folder = Some(Box::new(move |path| *answer.borrow_mut() = path.to_string()));
        perform(&mut h.app, e.clone(), Action::Reveal);
        assert_eq!(&*shown.borrow(), e.path.to_str().unwrap());
        assert!(e.path.exists());
    }
}
