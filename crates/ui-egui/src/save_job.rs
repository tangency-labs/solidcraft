//! Save and Save As off the window's thread (#3044): writing a large design (and the model a
//! save keeps with it) takes long enough that the window must keep drawing meanwhile. The
//! command runs on a copy of the session on a worker; the design waits for it like it waits for
//! a file picker, and the outcome is taken up as running the command here would have left it.

use serde_json::Value;
use solidcraft_engine::Session;

use crate::SolveApp;

/// Completion of a host-backed file write.
pub type PersistFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>>>>;

pub(crate) struct HostSave {
    command: &'static str,
    future: PersistFuture,
    pub(crate) reply: Option<(std::sync::mpsc::Sender<Value>, Value)>,
}

/// A save being written on a worker thread.
pub struct Saving {
    command: &'static str,
    done: std::sync::mpsc::Receiver<(Session, Result<Value, String>)>,
}

/// Run `file.save` or `file.save_as` with `params`. On the desktop it runs on a worker and is
/// taken up by [`tick`] (true: it is under way); in the browser, or when no thread can start, it
/// runs here and is finished on return (false).
pub fn start(app: &mut SolveApp, command: &'static str, params: Value) -> bool {
    // An assembly reading its links for a part edited in place takes them up when the read is
    // done: a save before then would record a design that is about to change as saved.
    if let Some(name) = crate::edit_in_place::refreshing_assembly(app) {
        app.set_status(crate::i18n::format("ui.edit_in_place_save_waits", &[("name", &name)]), true);
        return false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if app.saving.is_none() && app.file_picker.is_none() && !app.file_load.busy() {
        let mut copy = app.session.scratch();
        // The copy has no file: the command saves to this session's (and names the design after it).
        copy.path.clone_from(&app.session.path);
        let (tx, rx) = std::sync::mpsc::channel();
        let given = params.clone();
        let job = std::thread::Builder::new().name("save".into()).spawn(move || {
            let r = copy.execute(command, &given).map_err(|e| e.to_string());
            let _ = tx.send((copy, r));
        });
        if job.is_ok() {
            app.saving = Some(Saving { command, done: rx });
            return true;
        }
        // No thread to write on: save here, as before.
    }
    let result = app.run(command, params);
    if result.is_ok() && begin_host_save(app, command) {
        return true;
    }
    false
}

pub(crate) fn begin_programmatic(app: &mut SolveApp, command: &'static str) {
    if crate::webdav::managed(app) {
        app.webdav.interactive.set(false);
        begin_host_save_inner(app, command);
    }
}

fn begin_host_save(app: &mut SolveApp, command: &'static str) -> bool {
    app.webdav.interactive.set(true);
    app.host_save.is_some() || begin_host_save_inner(app, command)
}

fn begin_host_save_inner(app: &mut SolveApp, command: &'static str) -> bool {
    let _language = crate::webdav::managed(app).then(|| crate::i18n::scope(&app.preferences.language));
    if let Some(path) = app.session.path.clone()
        && let Some(future) = crate::webdav::persist(app, &path).or_else(|| app.services.persist_save.as_ref().and_then(|persist| persist(&path)))
    {
        app.session.mark_unsaved();
        app.host_save = Some(HostSave { command, future, reply: None });
        return true;
    }
    false
}

/// Take up a finished save (`wait`: wait for a running one).
pub fn tick(app: &mut SolveApp, wait: bool) {
    let _language = crate::webdav::managed(app).then(|| crate::i18n::scope(&app.preferences.language));
    if let Some(pending) = app.host_save.as_mut() {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let std::task::Poll::Ready(result) = pending.future.as_mut().poll(&mut context) else { return };
        let command = pending.command;
        let reply = pending.reply.take();
        app.host_save = None;
        if result.is_ok() {
            app.session.mark_saved();
        }
        if let Some((reply, success)) = reply {
            let answer = match &result {
                Ok(()) => success,
                Err(error) => serde_json::json!({"ok": false, "error": error}),
            };
            let _ = reply.send(answer);
        }
        app.finish_command(command, &result.map(|()| serde_json::json!({})));
        crate::documents::saved_for_close(app);
    }
    let Some(saving) = app.saving.as_ref() else { return };
    let outcome = if wait {
        saving.done.recv().ok()
    } else {
        match saving.done.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        }
    };
    let Some(Saving { command, .. }) = app.saving.take() else { return };
    let r = match outcome {
        Some((copy, Ok(v))) => {
            app.session.adopt_save(&copy);
            Ok(v)
        }
        Some((_, Err(e))) => Err(e),
        None => Err(format!("{command}: the save stopped")),
    };
    if r.is_ok() && begin_host_save(app, command) {
        return;
    }
    app.finish_command(command, &r);
    crate::documents::saved_for_close(app);
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn webdav_save_uses_preconditions_and_keeps_conflicts_unsaved() {
        use crate::webdav::{Config, Response, Target, condition, connection};
        let mut h = crate::scenario::Harness::new();
        assert!(!h.app.preferences.webdav_enabled);
        h.app.webdav.password = "original-secret".into();
        h.app.prefs_window.open = true;
        h.frames(2);
        h.app.webdav.password = "changed-secret".into();
        crate::prefs::close(&mut h.app, true);
        assert_eq!(h.app.webdav.password, "original-secret");
        assert!(!serde_json::to_string(&h.app.preferences).unwrap().contains("original-secret"));
        assert_eq!(condition(None, false), vec![("If-None-Match".into(), "*".into())]);
        assert_eq!(condition(Some("\"old\""), true), Vec::<(String, String)>::new());
        let url = connection("https://solidcraft.io/dav/").unwrap();
        let path = h.dir.join("remote.solidcraft").to_string_lossy().into_owned();
        h.app.run("solid.box", json!({"length": 2, "width": 2, "height": 2})).unwrap();
        h.app.run("file.save_as", json!({"path": path})).unwrap();
        h.app.webdav.targets.borrow_mut().insert(
            path.clone(),
            Target { config: Config { base: url.clone(), user: "test".into(), password: String::new() }, url, etag: Some("\"old\"".into()) },
        );
        h.app.services.webdav_http = Some(std::rc::Rc::new(|request| {
            assert_eq!(request.method, "PUT");
            assert_eq!(request.headers, vec![("If-Match".into(), "\"old\"".into())]);
            assert!(!request.body.is_empty());
            Box::pin(async { Ok(Response { status: 412, etag: None, body: Vec::new() }) })
        }));
        assert!(super::begin_host_save(&mut h.app, "file.save"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while h.app.host_save.is_some() {
            assert!(std::time::Instant::now() < deadline);
            super::tick(&mut h.app, false);
            std::thread::yield_now();
        }
        assert!(h.app.session.is_dirty());
        assert!(*h.app.webdav.conflict.borrow());
        assert_eq!(h.app.webdav.targets.borrow().get(&path).unwrap().etag.as_deref(), Some("\"old\""));
        assert!(crate::webdav::strong_etag(Some("W/\"weak\"".into())).is_err());
        *h.app.webdav.conflict.borrow_mut() = false;
        let (request, response) = crate::control::ControlRequest::new("engine.execute", json!({"command": "file.save", "params": {}}));
        h.app.preferences.language = "ja".into();
        h.app.run_control(&h.ctx, request);
        assert!(response.try_recv().is_err(), "the reply waits for the remote write");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while h.app.host_save.is_some() {
            assert!(std::time::Instant::now() < deadline);
            super::tick(&mut h.app, false);
            std::thread::yield_now();
        }
        let reply = response.recv().unwrap();
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["error"], crate::i18n::Locale::for_language("ja").text("webdav.error.conflict"));
        h.app.preferences.language = "en".into();
        assert!(h.app.session.is_dirty());
        assert!(!*h.app.webdav.conflict.borrow(), "programmatic failures never open a dialog");
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = calls.clone();
        h.app.services.webdav_http = Some(std::rc::Rc::new(move |request| {
            let call = seen.get();
            seen.set(call + 1);
            if call == 0 {
                assert!(request.headers.is_empty(), "explicit overwrite skips the condition once");
            } else {
                assert_eq!(request.headers, vec![("If-Match".into(), "\"new\"".into())]);
            }
            Box::pin(async { Ok(Response { status: 204, etag: Some("\"new\"".into()), body: Vec::new() }) })
        }));
        h.app.webdav.overwrite = true;
        for _ in 0..2 {
            assert!(super::begin_host_save(&mut h.app, "file.save"));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while h.app.host_save.is_some() {
                assert!(std::time::Instant::now() < deadline);
                super::tick(&mut h.app, false);
                std::thread::yield_now();
            }
            assert!(!h.app.session.is_dirty());
        }
        assert_eq!(calls.get(), 2);
        assert!(!h.app.webdav.overwrite);

        h.app.preferences.webdav_enabled = false;
        let reached = std::rc::Rc::new(std::cell::Cell::new(false));
        let called = reached.clone();
        h.app.services.pick_open = Some(Box::new(move |_| {
            called.set(true);
            Box::pin(async { None })
        }));
        h.app.pick_open(|_, _| {});
        assert!(reached.get(), "the disabled value preserves the host picker");
    }

    #[test]
    fn a_pending_save_blocks_feature_computation_and_completed_transaction_adoption() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("sketch.create", json!({"plane":"XY"})).unwrap();
        h.app.run("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[10,10]})).unwrap();
        h.app.finish_sketch();
        h.app.start("solid.extrude");
        h.frames(2);
        let dialog = h.app.dialog.as_ref().unwrap().clone();
        let cmds = crate::dialogs::apply_commands(&h.app, &dialog).unwrap();
        let prepared = h.app.session.scratch().compute_commands(&cmds, None).unwrap();
        let document = h.app.session.doc.clone();
        let history = h.app.session.undo.len();
        let (_save_finish, save_done) = std::sync::mpsc::channel();
        h.app.saving = Some(super::Saving { command: "file.save", done: save_done });
        h.app.apply_off_thread = Some(std::time::Duration::ZERO);
        assert!(crate::dialogs::begin_applying(&h.app, &dialog).is_none());
        assert!(crate::dialogs::run_dialog(&mut h.app, &dialog).unwrap_err().contains("file operation"));
        let (finish, done) = std::sync::mpsc::channel();
        h.app.applying = Some(crate::dialogs::Applying {
            done,
            ready: None,
            seen: true,
            started: crate::now_ms(),
            cmds,
            progress: Default::default(),
            quality: Default::default(),
        });
        finish.send(Ok(prepared)).ok().unwrap();
        // Run just the dialog, without the normal frame's save polling.
        let mut out = h.ctx.run_ui(Default::default(), |ui| crate::dialogs::show(&mut h.app, ui.ctx()));
        // Not drawn: discard the texture updates explicitly.
        out.textures_delta.clear();
        assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
        assert_eq!(h.app.session.undo.len(), history);
        assert!(h.app.dialog.as_ref().unwrap().error.as_ref().unwrap().contains("file operation"));
    }

    /// #6003: an agent's `engine.script` that arrives while the window's Save is being written
    /// waits for the save, as the other design-changing requests do, and runs after it: the
    /// file has the design as it was when Save was pressed, and the script's change is left
    /// unsaved. Before, the script ran at once, and taking up the save marked its change saved.
    #[test]
    fn a_script_waits_for_a_save_being_written() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let path = h.dir.join("Plate.solidcraft");
        // The save, written on a worker, finishes when the test says.
        let mut copy = h.app.session.scratch();
        copy.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
        let (finish, done) = std::sync::mpsc::channel();
        h.app.saving = Some(super::Saving { command: "file.save_as", done });
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.control_rx = Some(rx);
        let (req, reply) = crate::control::ControlRequest::new("engine.script", json!([{"command": "solid.sphere", "params": {"diameter": 4}}]));
        tx.send(req).unwrap();
        h.app.drain_control(&h.ctx);
        assert!(reply.try_recv().is_err(), "held while the save is written");
        assert_eq!(h.app.session.doc.features.len(), 1);
        finish.send((copy, Ok(json!({})))).unwrap();
        super::tick(&mut h.app, false);
        assert!(h.app.saving.is_none() && !h.app.session.is_dirty());
        h.app.drain_control(&h.ctx);
        assert!(reply.try_recv().is_ok(), "run once the save is taken up");
        assert_eq!(h.app.session.doc.features.len(), 2);
        assert!(h.app.session.is_dirty(), "the script's change is not in the file");
    }

    /// #6003: a save is taken up as saved only for the design it wrote. A design changed while
    /// the save was being written keeps its file and name from the save, but stays unsaved.
    #[test]
    fn a_design_changed_during_its_save_stays_unsaved() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let path = h.dir.join("Plate.solidcraft");
        let mut copy = h.app.session.scratch();
        copy.execute("file.save_as", &json!({"path": path.to_string_lossy()})).unwrap();
        let (finish, done) = std::sync::mpsc::channel();
        h.app.saving = Some(super::Saving { command: "file.save_as", done });
        // A change that reaches the session past the window's guards.
        h.app.session.execute("solid.sphere", &json!({"diameter": 4})).unwrap();
        finish.send((copy, Ok(json!({})))).unwrap();
        super::tick(&mut h.app, false);
        assert_eq!(h.app.session.path.as_deref(), path.to_str());
        assert_eq!(h.app.session.doc.name, "Plate");
        assert!(h.app.session.is_dirty(), "the sphere is not in the file");
    }

    #[test]
    fn host_save_waits_for_acknowledgement_and_failure_keeps_the_design_open() {
        use std::{cell::RefCell, rc::Rc, task::Poll};
        for success in [false, true] {
            let mut h = crate::scenario::Harness::new();
            h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
            let answer = Rc::new(RefCell::new(None));
            let reply = answer.clone();
            h.app.services.persist_save = Some(Box::new(move |_| {
                let reply = reply.clone();
                Some(Box::pin(std::future::poll_fn(move |_| reply.borrow_mut().take().map_or(Poll::Pending, Poll::Ready))))
            }));
            let path = h.dir.join("server.solidcraft");
            h.app.session.path = Some(path.to_string_lossy().into_owned());
            crate::documents::request_quit(&mut h.app);
            crate::documents::answer_prompt(&mut h.app, crate::documents::Answer::Save);
            super::tick(&mut h.app, true);
            assert!(h.app.host_save.is_some());
            assert!(h.app.file_picker_busy() && h.app.session.is_dirty());
            assert!(h.app.run("solid.box", json!({"length": 5, "width": 5, "height": 5})).is_err());
            crate::documents::request_quit(&mut h.app);
            assert!(!h.app.quit_requested);
            *answer.borrow_mut() = Some(if success { Ok(()) } else { Err("HTTP 412".into()) });
            super::tick(&mut h.app, false);
            assert!(!h.app.file_picker_busy());
            assert_eq!(h.app.session.is_dirty(), !success);
            assert_eq!(h.app.quit_requested, success);
        }
    }

    /// #3044: Save writes on a worker while the window keeps drawing; the design waits (commands
    /// are refused) until the save is taken up, which leaves it saved under its file, in the
    /// recent files, with the command echoed, as a save run here does.
    #[test]
    fn save_writes_on_a_worker_and_is_taken_up_like_a_save_here() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let path = h.dir.join("Plate.solidcraft");
        assert!(super::start(&mut h.app, "file.save_as", json!({"path": path.to_string_lossy()})), "under way on a worker");
        assert!(h.app.file_picker_busy() && h.app.run("solid.sphere", json!({"diameter": 4})).is_err(), "the design waits for it");
        h.frame();
        h.settle();
        assert!(h.app.saving.is_none() && !h.app.session.is_dirty());
        assert_eq!(h.app.session.path.as_deref(), path.to_str());
        assert_eq!(h.app.session.doc.name, "Plate");
        assert!(h.app.home.recent.iter().any(|r| r.as_str() == path.to_str().unwrap()), "{:?}", h.app.home.recent);
        assert!(h.app.session.log.iter().any(|l| l.contains("Save As")), "{:?}", h.app.session.log);
        // A failing save reports why, and leaves the changes unsaved.
        h.app.run("solid.sphere", json!({"diameter": 4})).unwrap();
        let bad = h.dir.join("missing").join("x.solidcraft");
        super::start(&mut h.app, "file.save_as", json!({"path": bad.to_string_lossy()}));
        super::tick(&mut h.app, true);
        assert!(h.app.session.is_dirty() && h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
    }
}
