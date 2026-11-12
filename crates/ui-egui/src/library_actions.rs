//! Thumbnail menu actions share the engine commands used by scripts and other frontends.
use super::{Entry, Highlight, open_entry};
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
        Action::Open => solidcraft_ui_text::tr("library.action_open"),
        Action::Insert => solidcraft_ui_text::tr("library.action_insert"),
        Action::Rename => solidcraft_ui_text::tr("library.action_rename"),
        Action::Move => solidcraft_ui_text::tr("library.action_move"),
        Action::Copy => solidcraft_ui_text::tr("library.action_copy"),
        Action::Trash => solidcraft_ui_text::tr("library.action_trash"),
        Action::Reveal => solidcraft_ui_text::tr("library.action_reveal"),
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
            if action == Action::Reveal {
                crate::ui_kit::Menu::button(ui, app.tr("library.highlight"), |ui| {
                    ui.set_min_width(140.0);
                    let current = app.library.highlights.get(&entry.path).copied();
                    for choice in std::iter::once(None).chain(Highlight::ALL.into_iter().map(Some)) {
                        let label = app.tr(choice.map(Highlight::label).unwrap_or("library.highlight_none"));
                        let response = crate::ui_kit::list_picker(ui, label, label, current == choice);
                        if let Some(color) = choice {
                            let rect = response.rect;
                            ui.painter().rect_filled(
                                egui::Rect::from_min_size(rect.right_top() + egui::vec2(-12.0, 6.0), egui::vec2(4.0, 12.0)),
                                1.0,
                                color.color(),
                            );
                        }
                        if response.clicked() {
                            app.library.set_highlight(&entry.path, choice);
                            ui.close();
                        }
                    }
                });
            }
            let path = entry.path.to_str();
            let enabled = path.is_some()
                && !app.file_picker_busy()
                && !app.file_load.busy()
                && (!matches!(action, Action::Rename | Action::Move | Action::Copy | Action::Trash) || app.file_load.idle())
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
        app.set_status(solidcraft_ui_text::tr("library.utf8"), true);
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
    if app.file_picker_busy() || !app.file_load.idle() {
        return false;
    }
    let Some(path) = entry.path.to_str() else { return false };
    let path = path.to_owned();
    let command = match action {
        Action::Copy => "file.copy",
        Action::Trash => "file.trash",
        _ => "file.move",
    };
    let params = json!({"path":path, "destination":destination});
    let guards = crate::documents::file_guard_sessions(app);
    let command_check = app.session.command_check();
    let scope = solidcraft_engine::io::vfs::current_scope();
    let (tx, rx) = std::sync::mpsc::channel();
    let protected = solidcraft_ui_text::tr("library.close_using_file").to_owned();
    let language = solidcraft_ui_text::language();
    let spawned = std::thread::Builder::new().name("library-file-action".into()).spawn(move || {
        let _language = solidcraft_ui_text::scope(language);
        let result = solidcraft_engine::io::vfs::scoped(scope, || {
            if action != Action::Copy && guards.iter().any(|s| solidcraft_engine::session_uses_file(s, &path)) {
                return (false, Err(protected));
            }
            let mut session = solidcraft_engine::Session::default();
            session.set_command_check(command_check);
            (true, session.execute(command, &params).map_err(|e| e.to_string()))
        });
        let _ = tx.send(result);
    });
    if let Err(error) = spawned {
        app.library.error = Some(solidcraft_ui_text::format("library.worker_start_failed", &[("error", &error.to_string())]));
        return false;
    }
    let entry = entry.clone();
    let destination = destination.map(std::path::Path::to_path_buf);
    app.set_status(solidcraft_ui_text::tr("library.file_working"), false);
    app.begin_file_task(
        std::future::poll_fn(move |_| match rx.try_recv() {
            Ok(result) => std::task::Poll::Ready(result),
            Err(std::sync::mpsc::TryRecvError::Empty) => std::task::Poll::Pending,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                std::task::Poll::Ready((true, Err(solidcraft_ui_text::tr("library.worker_stopped").into())))
            }
        }),
        move |app, (attempted, result)| {
            let _language = solidcraft_ui_text::scope(&app.preferences.language);
            // A partial transfer may have written a destination even on failure.
            if attempted {
                app.library.refresh();
            }
            finish(app, &entry, action, destination.as_deref(), result);
        },
    );
    true
}

fn finish(
    app: &mut SolveApp,
    entry: &Entry,
    action: Action,
    destination: Option<&std::path::Path>,
    result: Result<serde_json::Value, String>,
) -> bool {
    let path = entry.path.to_str().unwrap_or_default();
    match result {
        Ok(_) => {
            if action != Action::Copy
                && let Some(color) = app.library.highlights.remove(&entry.path)
                && let Some(destination) = destination
            {
                app.library.set_highlight(destination, Some(color));
            }
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
            app.set_status(solidcraft_ui_text::format("library.completed", &[("action", label(action))]), false);
            true
        }
        Err(error) => {
            app.set_status(error.clone(), true);
            app.library.error = Some(error);
            false
        }
    }
}

pub(super) fn dialog(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut dialog) = app.library.action_dialog.take() else { return };
    let mut confirm = false;
    let mut cancel = false;
    let (closed, action) = crate::ui_kit::Window::new(egui::Id::unique("library_file_action"), "folder", label(dialog.action))
        .actions(label(dialog.action), solidcraft_ui_text::tr("ui.cancel"), None)
        .fields()
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, true, |ui| {
            ui.label(&dialog.entry.name);
            if dialog.action == Action::Rename {
                let input = crate::ui_kit::bounded_cell_editor(
                    ui,
                    ui.scope_id().with("library_filename"),
                    &mut dialog.name,
                    "library.new_filename",
                    dialog.error.is_some(),
                    255,
                );
                input.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, solidcraft_ui_text::tr("library.new_filename")));
                if dialog.focus {
                    input.request_focus();
                    dialog.focus = false;
                }
                confirm |= input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            } else {
                ui.label(solidcraft_ui_text::tr("library.confirm_trash"));
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
                dialog.error = Some(solidcraft_ui_text::tr("library.invalid_name").into());
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

    fn finish_job(h: &mut Harness) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while h.app.file_picker_busy() && std::time::Instant::now() < deadline {
            crate::file_picker::tick(&mut h.app, &h.ctx);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!h.app.file_picker_busy());
    }

    #[test]
    fn waiting_import_keeps_its_source_safe_from_library_mutations() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.preferences.file_operations_wait = false;
        let e = entry(&h);
        let mut source = solidcraft_engine::Session::default();
        source.execute("solid.box", &json!({"length": 1, "width": 1, "height": 1})).unwrap();
        source.execute("file.save", &json!({"path": e.path})).unwrap();
        let before = std::fs::read(&e.path).unwrap();
        h.app.run("component.rename", json!({"component": 0, "name": "Assembly"})).unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        h.app.file_load.worker_gate = Some((started_tx, release_rx));
        crate::file_load::begin_request(&mut h.app, crate::file_load::Request::new(e.path.to_str().unwrap(), crate::file_load::Kind::InsertDesign))
            .unwrap();
        started_rx.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
        crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
        release_tx.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !crate::operations::current(&h.app).iter().any(|op| op.waiting) && std::time::Instant::now() < deadline {
            crate::file_load::tick(&mut h.app, &h.ctx);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(crate::operations::current(&h.app).iter().any(|op| op.waiting));
        assert!(!h.app.file_load.busy() && !h.app.file_load.idle());
        let destination = h.dir.join("Moved.solidcraft");
        for action in [Action::Rename, Action::Move, Action::Trash, Action::Copy] {
            assert!(!execute(&mut h.app, &e, action, Some(&destination)), "waiting imports retain the file-action lock");
            assert!(!h.app.file_picker_busy());
            assert_eq!(std::fs::read(&e.path).unwrap(), before);
            assert!(!destination.exists());
        }
        crate::documents::switch(&mut h.app, 0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !h.app.file_load.idle() && std::time::Instant::now() < deadline {
            crate::file_load::tick(&mut h.app, &h.ctx);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(h.app.file_load.idle());
        assert!(crate::documents::uses_file(&h.app, e.path.to_str().unwrap()));
        assert!(execute(&mut h.app, &e, Action::Rename, Some(&destination)));
        finish_job(&mut h);
        assert_eq!(std::fs::read(&e.path).unwrap(), before);
        assert!(!destination.exists(), "the adopted link now protects its source");
    }

    #[test]
    fn background_file_actions_honor_host_command_restrictions() {
        let mut h = Harness::new();
        let e = entry(&h);
        let before = std::fs::read(&e.path).unwrap();
        let destination = h.dir.join("Denied.sc");
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorded = calls.clone();
        h.app.session.set_command_check(Some(std::sync::Arc::new(move |command, _, _| {
            recorded.lock().unwrap().push(command.to_owned());
            Err("Host denied this file action".into())
        })));
        for action in [Action::Copy, Action::Move, Action::Trash] {
            assert!(execute(&mut h.app, &e, action, Some(&destination)));
            finish_job(&mut h);
            assert_eq!(std::fs::read(&e.path).unwrap(), before);
            assert!(!destination.exists());
            assert!(h.app.library.error.as_deref().is_some_and(|e| e.contains("Host denied")));
        }
        assert_eq!(*calls.lock().unwrap(), ["file.copy", "file.move", "file.trash"]);
    }

    #[test]
    fn background_file_lock_keeps_frames_responsive_and_defers_opens() {
        let mut h = Harness::new();
        let e = entry(&h);
        let before = h.app.session.doc.to_json();
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.begin_file_task(
            std::future::poll_fn(move |_| match rx.try_recv() {
                Ok(()) => std::task::Poll::Ready(()),
                _ => std::task::Poll::Pending,
            }),
            |_, ()| {},
        );
        for width in [960, 1280] {
            h.call("ui.resize", json!({"width":width,"height":720}));
            h.frames(2);
            assert_eq!(h.ctx.content_rect().width(), width as f32);
            assert!(h.app.file_picker_busy());
            assert!(h.app.run("file.trash", json!({"path":e.path})).is_err());
            assert!(!execute(&mut h.app, &e, Action::Copy, Some(&h.dir.join("No.sc"))));
            crate::documents::new_design(&mut h.app);
            crate::documents::request_quit(&mut h.app);
            assert_eq!(h.app.session.doc.to_json(), before);
            assert!(!h.app.quit_requested);
        }
        crate::file_load::begin(&mut h.app, e.path.to_str().unwrap(), false);
        h.frames(2);
        assert!(h.app.file_load.features().is_none(), "queued opens must not start behind a file mutation");
        assert!(h.app.session.path.is_none());
        tx.send(()).unwrap();
        finish_job(&mut h);
    }

    fn entry(h: &Harness) -> Entry {
        let path = h.dir.join("MenuTest.solidcraft");
        std::fs::write(&path, solidcraft_engine::io::write_design(&solidcraft_engine::doc::Document::new("Menu Test"))).unwrap();
        Entry { path, name: "MenuTest.solidcraft".into(), folder: false }
    }

    #[test]
    fn highlight_submenu_sets_replaces_and_clears_without_loading_or_writing_design() {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.services.pick_folder = Some(Box::new(|| Box::pin(std::future::ready(None))));
        let e = entry(&h);
        let before = std::fs::read(&e.path).unwrap();
        h.app.library.set_root(h.dir.clone());
        for _ in 0..200 {
            h.app.library.poll(&h.ctx);
            if !h.app.library.loading() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for choice in [Some(Highlight::Red), Some(Highlight::Purple), None] {
            h.frames(3);
            h.step(&json!({"click":{"handle":"library:entry:0"},"button":"right"})).unwrap();
            h.frames(2);
            let highlight_y = h.widget("Highlight").unwrap()[1];
            assert!(h.widget("Move to Trash").unwrap()[1] < highlight_y);
            assert!(highlight_y < h.widget("Show in Folder").unwrap()[1]);
            h.step(&json!({"widget":"Highlight"})).unwrap();
            h.frames(3);
            let label = solidcraft_ui_text::tr(choice.map(Highlight::label).unwrap_or("library.highlight_none"));
            assert!(h.widget(label).is_some(), "submenu must show {label}");
            h.step(&json!({"widget":label})).unwrap();
            h.frames(3);
            assert_eq!(h.app.library.highlights.get(&e.path).copied(), choice);
            assert_eq!(crate::scenario::rect_of("library:highlight_indicator").is_some(), choice.is_some());
            assert!(!h.app.file_load.busy());
            assert!(h.app.session.path.is_none());
            assert_eq!(std::fs::read(&e.path).unwrap(), before);
        }
    }

    #[test]
    fn successful_move_preserves_highlight_but_copy_does_not_steal_it() {
        let mut h = Harness::new();
        let e = entry(&h);
        h.app.library.set_highlight(&e.path, Some(Highlight::Green));
        let copied = h.dir.join("Copied.solidcraft");
        assert!(execute(&mut h.app, &e, Action::Copy, Some(&copied)));
        finish_job(&mut h);
        assert_eq!(h.app.library.highlights.get(&e.path), Some(&Highlight::Green));
        assert!(!h.app.library.highlights.contains_key(&copied));
        let moved = h.dir.join("Renamed.solidcraft");
        assert!(execute(&mut h.app, &e, Action::Rename, Some(&moved)));
        finish_job(&mut h);
        assert!(!h.app.library.highlights.contains_key(&e.path));
        assert_eq!(h.app.library.highlights.get(&moved), Some(&Highlight::Green));
        assert!(execute(&mut h.app, &e, Action::Rename, Some(&copied)));
        finish_job(&mut h);
        assert!(h.app.library.error.is_some());
        assert_eq!(h.app.library.highlights.get(&moved), Some(&Highlight::Green));
    }

    #[test]
    fn thumbnail_context_menu_has_supported_actions_and_escape_closes_it() {
        for mode in [crate::ui_kit::LayoutMode::Floating, crate::ui_kit::LayoutMode::Docked] {
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
        assert!(execute(&mut h.app, &e, Action::Trash, None));
        finish_job(&mut h);
        assert!(h.app.library.error.is_some());
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
        finish_job(&mut h);
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
