//! Open with EasyPrint: the UI side of `file.open_in_easyprint`. The command exports the bodies
//! and gives them to the host (`Services::easyprint_upload`), which uploads them to Printables
//! and answers on a channel; this opens the answer in the browser, or says why not. The model
//! leaves the machine, so the first time asks the user (a preference remembers the answer).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::SolveApp;
use crate::ui_kit;

/// What the host answers: the address to open, or why the upload failed.
pub type Answer = Result<String, String>;

/// Starts the upload of a file (its name, its bytes) and answers on the channel later.
pub type Upload = Arc<dyn Fn(&str, Vec<u8>, Sender<Answer>) + Send + Sync>;

/// The upload in flight, and the question waiting for an answer.
pub struct State {
    tx: Sender<Answer>,
    rx: Mutex<Receiver<Answer>>,
    /// An upload has been started and not answered. One at a time.
    busy: Arc<AtomicBool>,
    /// The parameters of a send waiting for the user to agree to the upload.
    ask: Option<Value>,
    /// The last address opened (a step of a scenario runs several frames, so tests can't catch it).
    #[cfg(test)]
    opened: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx: Mutex::new(rx),
            busy: Arc::default(),
            ask: None,
            #[cfg(test)]
            opened: None,
        }
    }
}

/// Give the session the host's way to upload (call after the services are set).
pub fn install(app: &mut SolveApp) {
    let Some(upload) = app.services.easyprint_upload.clone() else { return };
    let (tx, busy) = (app.easyprint.tx.clone(), app.easyprint.busy.clone());
    app.session.easyprint_upload = Some(Arc::new(move |name, bytes| {
        if busy.swap(true, Ordering::SeqCst) {
            return Err("the last model is still being uploaded to Printables".into());
        }
        upload(name, bytes, tx.clone());
        Ok(())
    }));
}

/// Whether this host can upload to EasyPrint.
pub fn available(app: &SolveApp) -> bool {
    app.services.easyprint_upload.is_some()
}

/// Send the bodies of `params` to EasyPrint: at once when the user has agreed before, else after
/// asking.
pub fn request(app: &mut SolveApp, params: Value) {
    if app.preferences.easyprint_consent {
        return go(app, params);
    }
    app.easyprint.ask = Some(params);
}

fn go(app: &mut SolveApp, mut params: Value) {
    let _language = crate::i18n::scope(&app.preferences.language);
    params["consent"] = json!(true);
    if app.run("file.open_in_easyprint", params).is_ok() {
        app.set_status(crate::i18n::tr("ui.easyprint_uploading"), false);
    }
}

/// Per frame: open the answer of an upload, or report its failure.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    let answer = app.easyprint.rx.lock().ok().and_then(|rx| rx.try_recv().ok());
    let Some(answer) = answer else {
        if app.easyprint.busy.load(Ordering::SeqCst) {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        return;
    };
    app.easyprint.busy.store(false, Ordering::SeqCst);
    let _language = crate::i18n::scope(&app.preferences.language);
    match answer {
        Ok(url) => {
            #[cfg(test)]
            {
                app.easyprint.opened = Some(url.clone());
            }
            ctx.open_url(egui::OpenUrl::new_tab(url));
            app.set_status(crate::i18n::tr("ui.easyprint_opened"), false);
        }
        Err(e) => app.set_status(crate::i18n::format("ui.easyprint_failed", &[("error", &e)]), true),
    }
}

/// The question: what is uploaded where, for how long.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(params) = app.easyprint.ask.clone() else { return };
    let _language = crate::i18n::scope(&app.preferences.language);
    let (closed, action) = ui_kit::Confirm::new(egui::Id::new("sc_easyprint_confirm"), "mesh_export", crate::i18n::tr_label("Open with EasyPrint"))
        .actions("Upload", "Cancel", None)
        .show(ctx, true, |ui| {
            ui_kit::text(ui, crate::i18n::tr("ui.easyprint_consent"), ui_kit::Text::Report("easyprint_consent", false));
        });
    match action {
        ui_kit::Action::Ok => {
            app.easyprint.ask = None;
            app.preferences.easyprint_consent = true;
            go(app, params);
        }
        ui_kit::Action::Cancel => app.easyprint.ask = None,
        _ if closed => app.easyprint.ask = None,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;
    use serde_json::json;
    use std::sync::Mutex;

    type Names = Arc<Mutex<Vec<String>>>;

    /// A host that records each upload's file name and answers `answer` at once, with the app set
    /// up for it and a box selected.
    fn harness(answer: Answer) -> (Harness, Names) {
        let mut h = Harness::new();
        h.app.run("solid.box", json!({"length": 2, "width": 3, "height": 4})).unwrap();
        h.app.run("select.set", json!({"items": [solidcraft_engine::Sel::Body { name: "Body1".into() }]})).unwrap();
        let names = Names::default();
        let log = names.clone();
        h.app.services.easyprint_upload = Some(Arc::new(move |name, _bytes, done| {
            log.lock().unwrap().push(name.to_string());
            done.send(answer.clone()).unwrap();
        }));
        install(&mut h.app);
        (h, names)
    }

    /// A few frames, then the address the app opened since the last call.
    fn opened(h: &mut Harness) -> Option<String> {
        h.frames(4);
        h.app.easyprint.opened.take()
    }

    fn status(h: &Harness) -> (String, bool) {
        h.app.status.clone().map(|(text, _, error)| (text, error)).unwrap_or_default()
    }

    #[test]
    fn the_first_upload_asks_and_nothing_goes_before_the_answer() {
        let (mut h, names) = harness(Ok("https://www.printables.com/slice?f=1".into()));
        h.app.start("file.open_in_easyprint");
        h.frames(2);
        h.step(&json!({"widget": "Send"})).unwrap();
        h.frames(3);
        assert!(names.lock().unwrap().is_empty(), "asked first");
        assert!(!h.app.preferences.easyprint_consent);
        h.step(&json!({"widget": "Cancel"})).unwrap();
        assert_eq!(opened(&mut h), None);
        assert!(names.lock().unwrap().is_empty() && h.app.easyprint.ask.is_none());
    }

    #[test]
    fn agreeing_uploads_the_model_and_opens_the_answer_and_the_next_one_does_not_ask() {
        let (mut h, names) = harness(Ok("https://www.printables.com/slice?f=1".into()));
        h.app.start("file.open_in_easyprint");
        h.frames(2);
        h.step(&json!({"widget": "Send"})).unwrap();
        h.frames(3);
        h.step(&json!({"widget": "Upload"})).unwrap();
        assert_eq!(opened(&mut h).as_deref(), Some("https://www.printables.com/slice?f=1"));
        assert!(h.app.preferences.easyprint_consent);
        let names = names.lock().unwrap().clone();
        assert!(names.len() == 1 && names[0].ends_with(".3mf"), "{names:?}");
        // Remembered: the next send goes at once.
        request(&mut h.app, json!({"bodies": ["Body1"]}));
        assert_eq!(opened(&mut h).as_deref(), Some("https://www.printables.com/slice?f=1"));
    }

    #[test]
    fn a_failed_upload_is_an_error_toast_and_opens_nothing() {
        let (mut h, _) = harness(Err("EasyPrint answered HTTP 500".into()));
        h.app.preferences.easyprint_consent = true;
        request(&mut h.app, json!({"bodies": ["Body1"]}));
        assert_eq!(opened(&mut h), None);
        let (text, error) = status(&h);
        assert!(error && text.contains("HTTP 500"), "{text}");
        // It can be tried again: the failure ended the upload.
        request(&mut h.app, json!({"bodies": ["Body1"]}));
        h.frames(2);
        assert!(status(&h).0.contains("HTTP 500"));
    }

    #[test]
    fn nothing_selected_or_no_host_is_an_error_and_uploads_nothing() {
        let (mut h, names) = harness(Ok("https://www.printables.com/slice".into()));
        h.app.preferences.easyprint_consent = true;
        request(&mut h.app, json!({"bodies": ["Nope"]}));
        assert_eq!(opened(&mut h), None);
        assert!(status(&h).1 && names.lock().unwrap().is_empty());
        let mut bare = Harness::new();
        assert!(!available(&bare.app));
        bare.app.start("file.open_in_easyprint");
        bare.frames(2);
        assert!(bare.app.dialog.is_none() && status(&bare).1, "unavailable, with a reason");
    }

    #[test]
    fn the_row_makes_a_command_without_a_slicer_and_a_browser_has_only_that_row() {
        let (mut h, _) = harness(Ok("https://www.printables.com/slice".into()));
        assert!(crate::dialogs_save_mesh::available(&h.app), "no program dialog, but EasyPrint");
        h.app.start("file.open_in_easyprint");
        h.frames(2);
        let Some(crate::dialogs::Kind::SaveMesh(k)) = h.app.dialog.as_ref().map(|d| &d.kind) else { panic!("no dialog") };
        let input = crate::selection::SelInput {
            items: vec![solidcraft_engine::Sel::Body { name: "Body1".into() }],
            ..crate::selection::SelInput::new("Bodies", crate::selection::BODIES, true)
        };
        let (id, p) = crate::dialogs_save_mesh::commands(k, &[input]).unwrap().remove(0);
        assert_eq!((id.as_str(), p.get("slicer"), p["format"].as_str()), ("file.open_in_easyprint", None, Some("3mf")));
    }
}
