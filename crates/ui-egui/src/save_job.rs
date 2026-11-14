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
    pub(crate) command: &'static str,
    future: PersistFuture,
    saved: Session,
    result: Value,
}

/// A save being written on a worker thread.
pub struct Saving {
    command: &'static str,
    done: std::sync::mpsc::Receiver<(Session, Result<Value, String>, bool)>,
    /// How far the file is written, and its Cancel: until the file is replaced, a cancelled save
    /// leaves the file and its backup as they were, and the design unsaved.
    pub(crate) progress: std::sync::Arc<solidcraft_engine::doc::progress::Progress>,
}

impl Saving {
    /// The command writing: `file.save` or `file.save_as`.
    pub(crate) fn command(&self) -> &'static str {
        self.command
    }
}

/// Run `file.save` or `file.save_as` with `params`. On the desktop it runs on a worker and is
/// taken up by [`tick`] (true: it is under way); in the browser, or when no thread can start, it
/// runs here. A host-backed write stays under way until the host acknowledges it (true).
pub fn start(app: &mut SolveApp, command: &'static str, params: Value) -> bool {
    // An assembly reading its links for a part edited in place takes them up when the read is
    // done: a save before then would record a design that is about to change as saved.
    if let Some(name) = crate::edit_in_place::refreshing_assembly(app) {
        app.set_status(solidcraft_ui_text::format("ui.edit_in_place_save_waits", &[("name", &name)]), true);
        return false;
    }
    if app.file_picker_busy() || app.file_load_holds() {
        return false;
    }
    // A save keeps print faces only when the user keeps them (#11512).
    crate::print_face_ui::sync(&mut app.session, &app.preferences);
    #[cfg(not(target_arch = "wasm32"))]
    if app.saving.is_none() && app.file_picker.is_none() && !app.file_load_holds() {
        let mut copy = app.session.scratch();
        // The copy has no file: the command saves to this session's (and names the design after it).
        copy.path.clone_from(&app.session.path);
        // This is the user's own Save: only this path may sign with the keyring.
        copy.signing_allowed = true;
        let (tx, rx) = std::sync::mpsc::channel();
        let given = params.clone();
        let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        let watching = progress.clone();
        let job = std::thread::Builder::new().name("save".into()).spawn(move || {
            let r = solidcraft_engine::doc::progress::watch(watching, || copy.execute(command, &given));
            // A cancelled save is the engine's cancellation, not an error: carry it as such
            // rather than reading the message again (the panel's outcome depends on it).
            let cancelled = matches!(&r, Err(solidcraft_engine::EngineError::Cancelled(_)));
            let r = r.map_err(|e| e.to_string());
            let _ = tx.send((copy, r, cancelled));
        });
        if job.is_ok() {
            app.saving = Some(Saving { command, done: rx, progress });
            return true;
        }
        // No thread to write on: save here, as before.
    }
    save_here(app, command, params)
}

fn save_here(app: &mut SolveApp, command: &'static str, params: Value) -> bool {
    // The browser and a failed worker launch use the same engine save, including provenance.
    let mut copy = app.session.scratch();
    copy.path.clone_from(&app.session.path);
    copy.signing_allowed = true;
    let result = copy.execute(command, &params);
    let cancelled = matches!(&result, Err(solidcraft_engine::EngineError::Cancelled(_)));
    if result.is_ok() {
        app.session.adopt_save(&copy);
    }
    complete(app, command, result.map_err(|e| e.to_string()), cancelled)
}

fn begin_host_save(app: &mut SolveApp, command: &'static str, result: Value) -> bool {
    if let Some(path) = app.session.path.as_deref()
        && let Some(future) = app.services.persist_save.as_ref().and_then(|persist| persist(path))
    {
        let mut saved = app.session.scratch();
        saved.path.clone_from(&app.session.path);
        app.session.mark_unsaved();
        app.host_save = Some(HostSave { command, future, saved, result });
        return true;
    }
    false
}

/// Take up a finished save (`wait`: wait for a running one).
pub fn tick(app: &mut SolveApp, wait: bool) {
    if let Some(pending) = app.host_save.as_mut() {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let std::task::Poll::Ready(result) = pending.future.as_mut().poll(&mut context) else { return };
        let Some(HostSave { command, saved, result: value, .. }) = app.host_save.take() else { return };
        if result.is_ok() {
            app.session.adopt_save(&saved);
        }
        finish(app, command, result.map(|()| value), false);
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
    let (r, cancelled) = match outcome {
        Some((copy, Ok(v), _)) => {
            app.session.adopt_save(&copy);
            (Ok(v), false)
        }
        Some((_, Err(e), cancelled)) => (Err(e), cancelled),
        None => (Err(format!("{command}: the save stopped")), false),
    };
    if complete(app, command, r, cancelled) {
        return;
    }
    crate::documents::saved_for_close(app);
}

/// Keep the engine's result until the host has durably written those same bytes.
fn complete(app: &mut SolveApp, command: &'static str, r: Result<Value, String>, cancelled: bool) -> bool {
    if let Ok(value) = &r
        && begin_host_save(app, command, value.clone())
    {
        return true;
    }
    finish(app, command, r, cancelled);
    false
}

fn finish(app: &mut SolveApp, command: &'static str, r: Result<Value, String>, cancelled: bool) {
    let outcome = match &r {
        Ok(_) => crate::operations::Outcome::Done,
        Err(_) if cancelled => crate::operations::Outcome::Cancelled,
        Err(e) => crate::operations::Outcome::Failed(crate::friendly_error(command, e)),
    };
    crate::operations::ended(app, &format!("save:{command}"), outcome);
    if cancelled {
        let name = solidcraft_ui_text::tr_label(crate::command_label(command)).to_string();
        app.set_status(solidcraft_ui_text::format("operations.cancelled", &[("name", &name)]), false);
    } else {
        app.finish_command(command, &r);
        // Preserve the engine's newer-format warnings after host persistence, too.
        if let Some(first) = r.as_ref().ok().and_then(|v| v["warnings"].as_array()).and_then(|w| w.first()).and_then(serde_json::Value::as_str) {
            app.set_status(first, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

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
        h.app.saving = Some(super::Saving { command: "file.save", done: save_done, progress: Default::default() });
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
        h.app.saving = Some(super::Saving { command: "file.save_as", done, progress: Default::default() });
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.control_rx = Some(rx);
        let (req, reply) = crate::control::ControlRequest::new("engine.script", json!([{"command": "solid.sphere", "params": {"diameter": 4}}]));
        tx.send(req).unwrap();
        h.app.drain_control(&h.ctx);
        assert!(reply.try_recv().is_err(), "held while the save is written");
        assert_eq!(h.app.session.doc.features.len(), 1);
        finish.send((copy, Ok(json!({})), false)).unwrap();
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
        h.app.saving = Some(super::Saving { command: "file.save_as", done, progress: Default::default() });
        // A change that reaches the session past the window's guards.
        h.app.session.execute("solid.sphere", &json!({"diameter": 4})).unwrap();
        finish.send((copy, Ok(json!({})), false)).unwrap();
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

    #[test]
    fn host_save_preserves_signature_warnings_and_only_marks_the_uploaded_design_saved() {
        use std::{cell::RefCell, rc::Rc, task::Poll};
        for changed in [false, true] {
            for warning in [false, true] {
                let mut h = crate::scenario::Harness::new();
                h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
                let answer = Rc::new(RefCell::new(None));
                let reply = answer.clone();
                h.app.services.persist_save = Some(Box::new(move |_| {
                    let reply = reply.clone();
                    Some(Box::pin(std::future::poll_fn(move |_| reply.borrow_mut().take().map_or(Poll::Pending, Poll::Ready))))
                }));
                let mut copy = h.app.session.scratch();
                copy.path = Some(h.dir.join("Signed.sc").to_string_lossy().into_owned());
                copy.provenance_chain = Some(solidcraft_engine::io::ProvenanceChain { records: vec![vec![1, 2, 3]], truncated: false });
                copy.mark_saved();
                let mut result = json!({"signature": {"user_id": "Folder signer", "fingerprint": "012345"}});
                if warning {
                    result["warnings"] = json!(["Newer design data was not kept"]);
                }
                let (finish, done) = std::sync::mpsc::channel();
                h.app.saving = Some(super::Saving { command: "file.save_as", done, progress: Default::default() });
                finish.send((copy, Ok(result), false)).unwrap();
                let echoes = h.app.session.log.len();
                super::tick(&mut h.app, false);
                assert!(h.app.host_save.is_some() && h.app.session.is_dirty());
                assert_eq!(h.app.session.log.len(), echoes, "no completed-save echo before acknowledgement");
                let listed = crate::operations::control(&mut h.app, &json!({})).unwrap();
                assert_eq!(listed["operations"][0]["key"], "save:file.save_as");
                assert_eq!(listed["operations"][0]["cancellable"], false);
                assert_eq!(listed["finished"].as_array().unwrap().len(), 0);
                if changed {
                    // A change bypassing the window guards is not part of the uploaded bytes.
                    h.app.session.execute("solid.sphere", &json!({"diameter": 4})).unwrap();
                }
                *answer.borrow_mut() = Some(Ok(()));
                super::tick(&mut h.app, false);
                assert!(h.app.host_save.is_none());
                assert_eq!(h.app.session.is_dirty(), changed);
                assert_eq!(h.app.session.provenance_chain.as_ref().unwrap().records, vec![vec![1, 2, 3]]);
                let (status, _, error) = h.app.status.as_ref().unwrap();
                if warning {
                    assert_eq!(status, "Newer design data was not kept");
                    assert!(*error);
                } else {
                    assert!(status.contains("Folder signer"), "{status}");
                    assert!(!error);
                }
                let listed = crate::operations::control(&mut h.app, &json!({})).unwrap();
                assert_eq!(listed["finished"][0]["outcome"], "done");
            }
        }
    }

    #[test]
    fn browser_save_uses_the_engine_save_before_waiting_for_the_host() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let path = h.dir.join("Browser.sc");
        let persisted = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = persisted.clone();
        let expected = path.clone();
        h.app.services.persist_save = Some(Box::new(move |path| {
            assert_eq!(std::path::Path::new(path), expected);
            let bytes = std::fs::read(path).unwrap();
            let design = solidcraft_engine::io::read_design(&bytes).unwrap();
            assert_eq!(design.name, "Browser");
            assert_eq!(design.features.len(), 1);
            seen.set(true);
            Some(Box::pin(std::future::ready(Ok(()))))
        }));
        assert!(super::save_here(&mut h.app, "file.save_as", json!({"path": path.to_string_lossy()})));
        assert!(persisted.get() && h.app.session.is_dirty());
        assert!(!h.app.session.signing_allowed, "only the save copy can use the signing key");
        super::tick(&mut h.app, false);
        assert!(!h.app.session.is_dirty());
        assert!(h.app.status.as_ref().unwrap().0.contains("Browser.sc"));
    }

    #[test]
    fn a_script_and_signature_verification_wait_for_a_host_save() {
        use std::{cell::RefCell, rc::Rc, task::Poll};
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let answer = Rc::new(RefCell::new(None));
        let reply = answer.clone();
        h.app.services.persist_save = Some(Box::new(move |_| {
            let reply = reply.clone();
            Some(Box::pin(std::future::poll_fn(move |_| reply.borrow_mut().take().map_or(Poll::Pending, Poll::Ready))))
        }));
        let path = h.dir.join("Uploaded.sc");
        assert!(super::save_here(&mut h.app, "file.save_as", json!({"path": path.to_string_lossy()})));
        assert!(!crate::verify_job::start(&mut h.app), "verification waits for the uploaded version");
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.control_rx = Some(rx);
        let (req, reply) = crate::control::ControlRequest::new("engine.script", json!([{"command": "solid.sphere", "params": {"diameter": 4}}]));
        tx.send(req).unwrap();
        h.app.drain_control(&h.ctx);
        assert!(reply.try_recv().is_err(), "held until the server acknowledges the save");
        assert_eq!(h.app.session.doc.features.len(), 1);
        *answer.borrow_mut() = Some(Ok(()));
        super::tick(&mut h.app, false);
        assert!(!h.app.session.is_dirty());
        h.app.drain_control(&h.ctx);
        assert!(reply.try_recv().is_ok());
        assert_eq!(h.app.session.doc.features.len(), 2);
        assert!(h.app.session.is_dirty(), "the script changed the design after it was saved");
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

    #[test]
    fn an_unsigned_save_of_a_signed_design_says_the_earlier_signature_does_not_cover_it() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.session.path = Some("/work/Signed.sc".into());
        h.app.session.provenance_chain = Some(solidcraft_engine::io::ProvenanceChain { records: vec![vec![1, 2, 3]], truncated: false });
        h.app.finish_command("file.save", &Ok(json!({"path": "/work/Signed.sc"})));
        let (text, _, error) = h.app.status.as_ref().expect("the save says where it went");
        assert!(text.contains("earlier signature") && text.contains("does not cover"), "{text}");
        assert!(!error);
    }

    /// A save can be cancelled until its file is replaced: a cancelled save writes nothing, and
    /// the design keeps its changes to save. From the operations panel (here `ui.operations`) its
    /// row's X stops it, and it ends cancelled, not as an error.
    #[test]
    fn a_save_cancelled_before_its_file_is_replaced_writes_nothing() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let path = h.dir.join("Cancelled.solidcraft");
        // The engine: a watched save asked to stop writes nothing.
        let stop = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        stop.cancel();
        let r = solidcraft_engine::doc::progress::watch(stop, || h.app.session.execute("file.save_as", &json!({"path": path.to_string_lossy()})));
        assert!(matches!(r, Err(solidcraft_engine::EngineError::Cancelled(_))), "{r:?}");
        assert!(!path.exists() && h.app.session.is_dirty() && h.app.session.path.is_none());
        // The window: a save under way is a row with a working X.
        let (finish, done) = std::sync::mpsc::channel();
        let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        h.app.saving = Some(super::Saving { command: "file.save_as", done, progress: progress.clone() });
        let listed = crate::operations::control(&mut h.app, &json!({})).unwrap();
        assert_eq!(listed["operations"][0]["key"], "save:file.save_as", "{listed}");
        assert_eq!(listed["operations"][0]["cancellable"], true, "{listed}");
        crate::operations::control(&mut h.app, &json!({"cancel": "save:file.save_as"})).unwrap();
        assert!(progress.is_cancelled(), "the worker is asked to stop");
        let listed = crate::operations::control(&mut h.app, &json!({})).unwrap();
        assert_eq!(listed["operations"][0]["cancelling"], true, "{listed}");
        h.frame();
        let shown = |h: &crate::scenario::Harness| {
            h.app.operations.rows(crate::now_ms()).find(|(r, _)| r.key == "save:file.save_as").and_then(|(r, _)| r.status())
        };
        assert_eq!(shown(&h), Some(solidcraft_ui_text::tr("operations.cancelling").to_string()), "drawn while it stops");
        // The worker stops before replacing the file: cancelled, not an error, still unsaved.
        finish.send((h.app.session.scratch(), Err(format!("{}: save cancelled", path.display())), true)).unwrap();
        super::tick(&mut h.app, false);
        assert!(h.app.saving.is_none() && h.app.session.is_dirty() && !path.exists());
        assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| !error), "{:?}", h.app.status);
        let listed = crate::operations::control(&mut h.app, &json!({})).unwrap();
        assert_eq!(listed["finished"][0]["outcome"], "cancelled", "{listed}");
        // Its row fades out saying it was cancelled, not still cancelling.
        h.frame();
        assert_eq!(shown(&h), Some(solidcraft_ui_text::tr("operations.stopped").to_string()));
    }
}
