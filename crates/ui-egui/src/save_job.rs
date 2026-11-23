//! Save and Save As off the window's thread (#3044): writing a large design (and the model a
//! save keeps with it) takes long enough that the window must keep drawing meanwhile. The
//! command runs on a copy of the session on a worker; the design waits for it like it waits for
//! a file picker, and the outcome is taken up as running the command here would have left it.

use serde_json::Value;
use solidcraft_engine::Session;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use crate::SolveApp;

/// A save being written on a worker thread.
pub struct Saving {
    command: &'static str,
    done: std::sync::mpsc::Receiver<(Session, Result<Value, String>, bool)>,
    /// How far the file is written, and its Cancel: until the file is replaced, a cancelled save
    /// leaves the file and its backup as they were, and the design unsaved.
    pub(crate) progress: std::sync::Arc<solidcraft_engine::doc::progress::Progress>,
}

pub type PersistDesign = Box<dyn Fn(String, Vec<u8>) -> Receiver<Result<(), String>>>;

pub(crate) struct HostSaving {
    command: &'static str,
    done: Receiver<Result<(), String>>,
    copy: Session,
    original_path: Option<String>,
    original_name: String,
    value: Value,
    reply: Option<Sender<Value>>,
    check: Option<solidcraft_engine::schema::ParamCheck>,
}

pub(crate) fn host_pending(app: &SolveApp, uid: u64) -> bool {
    app.host_saves.iter().any(|save| save.copy.uid == uid)
}

/// Whether a browser save is about to replace this path.
pub fn host_path_pending(app: &SolveApp, path: &str) -> bool {
    app.host_saves.iter().any(|save| save.copy.path.as_deref() == Some(path) || save.original_path.as_deref() == Some(path))
}

/// The browser's saved-file time cannot supersede this revision's kept copy.
pub fn protects_host_recovery(app: &SolveApp, session: &Session) -> bool {
    app.host_save_left_dirty.contains(&session.uid)
        || app
            .host_saves
            .iter()
            .any(|save| save.copy.uid == session.uid && (save.copy.revision != session.revision || save.copy.visibility != session.visibility))
}

impl Saving {
    /// The command writing: `file.save` or `file.save_as`.
    pub(crate) fn command(&self) -> &'static str {
        self.command
    }
}

/// Run `file.save` or `file.save_as` with `params`. On the desktop it runs on a worker and is
/// taken up by [`tick`]. A browser host acknowledges its own write through the same tick.
/// Returns true while the save is under way.
pub fn start(app: &mut SolveApp, command: &'static str, params: Value) -> bool {
    // An assembly reading its links for a part edited in place takes them up when the read is
    // done: a save before then would record a design that is about to change as saved.
    if let Some(name) = crate::edit_in_place::refreshing_assembly(app) {
        app.set_status(solidcraft_ui_text::format("ui.edit_in_place_save_waits", &[("name", &name)]), true);
        return false;
    }
    if app.services.persist_design.is_some() {
        return match start_host(app, command, params, None, None) {
            Ok(()) => true,
            Err(e) => {
                finish_result(app, command, &Err(e));
                false
            }
        };
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
    let r = app.run(command, params);
    offer_container(app, &r);
    false
}

/// Run the same save command as native on a snapshot, then ask the host to persist it.
pub(crate) fn start_host(
    app: &mut SolveApp,
    command: &'static str,
    params: Value,
    reply: Option<Sender<Value>>,
    check: Option<solidcraft_engine::schema::ParamCheck>,
) -> Result<(), String> {
    if app.file_picker_busy() || app.file_load_holds() || host_pending(app, app.session.uid) {
        return Err("a file operation is in progress".into());
    }
    let persist = app.services.persist_design.as_ref().ok_or("no host design storage")?;
    crate::print_face_ui::sync(&mut app.session, &app.preferences);
    let original_path = app.session.path.clone();
    let original_name = app.session.doc.name.clone();
    let mut copy = app.session.scratch();
    copy.path.clone_from(&original_path);
    if let Some(level) = check {
        copy.param_check = level;
    }
    let (value, bytes) = copy.prepare_host_save(command, &params).map_err(|e| e.to_string())?;
    let path = copy.path.clone().ok_or("no path for the prepared save")?;
    if host_path_pending(app, &path) {
        return Err("a save to this file is already in progress".into());
    }
    let done = persist(path, bytes);
    app.host_saves.push(HostSaving { command, done, copy, original_path, original_name, value, reply, check });
    Ok(())
}

fn tick_host(app: &mut SolveApp) {
    let mut i = 0;
    while i < app.host_saves.len() {
        let result = match app.host_saves[i].done.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                i += 1;
                continue;
            }
            Err(TryRecvError::Disconnected) => Err("the host save stopped".into()),
        };
        let save = app.host_saves.remove(i);
        finish_host(app, save, result);
    }
    if !app.host_save_left_dirty.is_empty() {
        let dirty = |s: &Session, uid| s.uid == uid && s.is_dirty();
        app.host_save_left_dirty.retain(|uid| dirty(&app.session, *uid) || app.docs.background().any(|s| dirty(s, *uid)));
    }
}

fn finish_host(app: &mut SolveApp, save: HostSaving, result: Result<(), String>) {
    let HostSaving { command, copy, original_name, value, reply, check, .. } = save;
    let owns = |session: &Session| session.uid == copy.uid && session.document_id == copy.document_id;
    let active = owns(&app.session);
    let mut dirty = false;
    let result = result.map(|()| {
        let owner =
            if active { Some(&mut app.session) } else { app.docs.background_mut().find(|slot| owns(&slot.session)).map(|slot| &mut slot.session) };
        if let Some(owner) = owner {
            owner.adopt_host_save(&copy, &original_name);
            dirty = owner.is_dirty();
            if dirty {
                app.host_save_left_dirty.insert(owner.uid);
            }
        }
        value
    });
    if active || result.is_err() {
        app.finish_command(command, &result);
        offer_container(app, &result);
    } else if let Some(path) = copy.path.as_deref() {
        app.home.add_recent(path);
        crate::home::remember_session(&mut app.home, &copy);
        let status = app.tr("ui.design_saved").replace("{path}", path);
        app.set_status(status, false);
    }
    if dirty && let Some(path) = copy.path.as_deref() {
        let status = app.tr("ui.browser_saved_earlier").replace("{path}", path);
        app.set_status(status, false);
    }
    show_warning(app, &result);
    if active {
        crate::documents::saved_for_close(app);
    }
    if let Some(reply) = reply {
        let response = match result {
            Ok(value) => {
                let mut response = serde_json::json!({"ok": true, "result": value});
                if let Some(level) = check {
                    response["param_check"] = serde_json::json!(level.name());
                }
                response
            }
            Err(error) => serde_json::json!({"ok": false, "error": error}),
        };
        let _ = reply.send(response);
    }
}

/// A JSON save refused because the design holds binary data (images, plugin files): its toast
/// offers Save as .sc, so that data is saved rather than left out.
fn offer_container<T>(app: &mut SolveApp, r: &Result<T, String>) {
    if r.as_ref().err().is_some_and(|e| e.contains(solidcraft_engine::io::JSON_CANT_CARRY)) && app.status.as_ref().is_some_and(|s| s.2) {
        app.status_save_sc = true;
    }
}

/// Take up a finished save (`wait`: wait for a running one).
pub fn tick(app: &mut SolveApp, wait: bool) {
    tick_host(app);
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
    let outcome = match &r {
        Ok(_) => crate::operations::Outcome::Done,
        Err(_) if cancelled => crate::operations::Outcome::Cancelled,
        Err(e) => crate::operations::Outcome::Failed(crate::friendly_error(command, e)),
    };
    crate::operations::ended(app, &format!("save:{command}"), outcome);
    if cancelled {
        // Not an error: the file is as it was, and the design still has its changes to save.
        let name = solidcraft_ui_text::tr_command(command, crate::command_label(command)).to_string();
        app.set_status(solidcraft_ui_text::format("operations.cancelled", &[("name", &name)]), false);
    } else {
        finish_result(app, command, &r);
    }
    crate::documents::saved_for_close(app);
}

fn finish_result(app: &mut SolveApp, command: &str, result: &Result<Value, String>) {
    app.finish_command(command, result);
    offer_container(app, result);
    show_warning(app, result);
}

fn show_warning(app: &mut SolveApp, result: &Result<Value, String>) {
    if let Some(first) = result.as_ref().ok().and_then(|v| v["warnings"].as_array()).and_then(|w| w.first()).and_then(Value::as_str) {
        app.set_status(first, true);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    type HostWrite = (String, Vec<u8>, std::sync::mpsc::Sender<Result<(), String>>);

    fn browser_writes(app: &mut crate::SolveApp) -> Rc<RefCell<Vec<HostWrite>>> {
        let writes = Rc::new(RefCell::new(Vec::new()));
        let pending = writes.clone();
        app.services.persist_design = Some(Box::new(move |path, bytes| {
            let (send, receive) = std::sync::mpsc::channel();
            pending.borrow_mut().push((path, bytes, send));
            receive
        }));
        writes
    }

    #[test]
    fn a_failed_browser_save_as_leaves_the_original_background_tab_dirty() {
        let mut h = crate::scenario::Harness::new();
        let writes = browser_writes(&mut h.app);
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        let uid = h.app.session.uid;
        let path = h.dir.join("Host.sc");
        assert!(super::start(&mut h.app, "file.save_as", json!({"path":path})));
        assert!(h.app.session.is_dirty());
        assert!(h.app.session.path.is_none() && !path.exists());
        assert!(h.app.home.recent.is_empty());
        crate::documents::new_tab(&mut h.app);
        assert_ne!(h.app.session.uid, uid, "the browser can switch designs while the host writes");
        let (sent_path, bytes, done) = writes.borrow_mut().pop().unwrap();
        assert_eq!(Some(sent_path.as_str()), path.to_str());
        assert_eq!(solidcraft_engine::io::read_design(&bytes).unwrap().features.len(), 1);
        done.send(Err("quota exceeded".into())).unwrap();
        super::tick(&mut h.app, false);
        let owner = crate::documents::open_sessions(&h.app).into_iter().find(|s| s.uid == uid).unwrap();
        assert!(owner.is_dirty() && owner.path.is_none());
        assert!(!h.app.session.is_dirty(), "the shown tab is unaffected");
        assert!(h.app.status.as_ref().is_some_and(|(message, _, error)| *error && message.contains("quota exceeded")));
        assert!(h.app.home.recent.is_empty());
    }

    #[test]
    fn a_browser_save_acknowledges_the_written_revision_of_its_own_tab() {
        let mut h = crate::scenario::Harness::new();
        let writes = browser_writes(&mut h.app);
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        let uid = h.app.session.uid;
        let path = h.dir.join("Host.sc");
        assert!(super::start(&mut h.app, "file.save_as", json!({"path":path})));
        h.app.run("solid.sphere", json!({"diameter":5})).unwrap();
        assert!(super::protects_host_recovery(&h.app, &h.app.session));
        crate::toolbar::save(&mut h.app);
        assert_eq!(writes.borrow().len(), 1);
        assert!(h.app.status.as_ref().is_some_and(|(message, _, error)| *error && message.contains("already in progress")));
        crate::documents::new_tab(&mut h.app);
        let (_, bytes, done) = writes.borrow_mut().pop().unwrap();
        assert_eq!(solidcraft_engine::io::read_design(&bytes).unwrap().features.len(), 1);
        done.send(Ok(())).unwrap();
        super::tick(&mut h.app, false);
        let owner = crate::documents::open_sessions(&h.app).into_iter().find(|s| s.uid == uid).unwrap();
        assert_eq!(owner.path.as_deref(), path.to_str());
        assert_eq!(owner.doc.features.len(), 2);
        assert!(owner.is_dirty() && super::protects_host_recovery(&h.app, owner));
        assert!(!h.app.session.is_dirty());
    }

    #[test]
    fn browser_save_and_close_waits_for_the_write_result() {
        for success in [false, true] {
            let mut h = crate::scenario::Harness::new();
            let writes = browser_writes(&mut h.app);
            h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
            let path = h.dir.join("Host.sc");
            let destination = path.to_string_lossy().into_owned();
            h.app.services.pick_save = Some(Box::new(move |_, _| Box::pin(std::future::ready(Some(destination.clone())))));
            crate::documents::close(&mut h.app, 0, false);
            crate::documents::answer_prompt(&mut h.app, crate::documents::Answer::Save);
            h.frames(2);
            assert!(h.app.file_picker_busy() && h.app.session.is_dirty());
            let (_, _, done) = writes.borrow_mut().pop().unwrap();
            done.send(if success { Ok(()) } else { Err("permission denied".into()) }).unwrap();
            super::tick(&mut h.app, false);
            assert!(!h.app.file_picker_busy());
            assert_eq!(h.app.session.is_dirty(), !success);
            assert_eq!(h.app.session.doc.features.is_empty(), success);
        }
    }

    #[test]
    fn a_browser_control_save_replies_only_after_persistence() {
        let mut h = crate::scenario::Harness::new();
        let writes = browser_writes(&mut h.app);
        h.app.run("solid.box", json!({"length":10,"width":10,"height":10})).unwrap();
        let path = h.dir.join("Host.sc");
        let save = json!({"command":"  FILE.SAVE_AS  ","params":{"path":path}});
        for (method, params) in [("engine.batch", json!({"commands":[save.clone()]})), ("engine.script", json!([save.clone()]))] {
            let (request, reply) = crate::control::ControlRequest::new(method, params);
            h.app.run_control(&h.ctx, request);
            assert!(reply.try_recv().unwrap()["error"].as_str().unwrap().contains("standalone"));
            assert!(writes.borrow().is_empty() && h.app.session.is_dirty());
        }
        let (request, reply) = crate::control::ControlRequest::new("engine.execute", save);
        h.app.run_control(&h.ctx, request);
        assert!(reply.try_recv().is_err());
        let (_, _, done) = writes.borrow_mut().pop().unwrap();
        done.send(Ok(())).unwrap();
        super::tick(&mut h.app, false);
        assert_eq!(reply.try_recv().unwrap()["ok"], true);
        assert!(!h.app.session.is_dirty());
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
            h.app.operations.rows(crate::operations::frame_ms(&h.ctx)).find(|(r, _)| r.key == "save:file.save_as").and_then(|(r, _)| r.status())
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

    /// A design holding binary data (here a plugin's file) can't be a plain JSON design: Save As
    /// .json is refused, writes nothing, keeps the design unsaved, and its toast offers Save as .sc.
    #[test]
    fn a_json_save_of_binary_data_is_refused_and_offers_a_container() {
        let mut h = crate::scenario::Harness::new();
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        h.app.session.doc_mut().plugin_data.insert("gears/blobs/table.bin".into(), vec![0, 1, 2, 255].into());
        let path = h.dir.join("Gears.json");
        super::start(&mut h.app, "file.save_as", json!({"path": path.to_string_lossy()}));
        super::tick(&mut h.app, true);
        assert!(!path.exists() && h.app.session.is_dirty(), "nothing written, nothing lost");
        let (text, _, error) = h.app.status.clone().unwrap();
        assert!(error && text.contains("plugin files"), "{text}");
        assert!(h.app.status_save_sc, "the toast offers Save as .sc");
        h.frame();
        // Any other message takes the offer away.
        h.app.set_status("Saved", false);
        assert!(!h.app.status_save_sc);
    }
}
