//! Checking a saved design's signature off the window's thread: verification reads every
//! authoritative part to hash it, which takes long enough on a large design that the window must
//! keep drawing meanwhile. The report is taken up by [`tick`] when the worker answers.

use std::sync::mpsc;

use solidcraft_engine::io::ProvenanceReport;

use crate::SolveApp;

/// A signature check running on a worker.
pub struct Verifying {
    /// The file it checks: an answer for another path (the tab changed meanwhile) is discarded.
    path: String,
    done: mpsc::Receiver<Option<ProvenanceReport>>,
}

/// Start checking the active design's signature. `false` when nothing started (there is no file,
/// a save is being written, or a check is already running).
pub fn start(app: &mut SolveApp) -> bool {
    if app.verifying.is_some() {
        return false;
    }
    if app.saving.is_some() || app.file_picker.is_some() || app.file_load_holds() {
        app.set_status(solidcraft_ui_text::tr("ui.verify_waits_for_file_operation").to_string(), true);
        return false;
    }
    let Some(path) = app.session.path.clone() else {
        app.set_status(solidcraft_ui_text::tr("ui.verify_needs_file").to_string(), true);
        return false;
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = mpsc::channel();
        let checked = path.clone();
        let spawned = std::thread::Builder::new().name("verify".into()).spawn(move || {
            let report = solidcraft_engine::io::read_design_provenance_file(&checked);
            let _ = tx.send(report);
        });
        if spawned.is_ok() {
            app.verifying = Some(Verifying { path, done: rx });
            app.set_status(solidcraft_ui_text::tr("ui.verify_checking").to_string(), false);
            return true;
        }
        // No thread to read on: check here, as the menu would have before there was a worker.
    }
    // The browser holds designs in memory (no threads): check here, which is quick.
    let report = solidcraft_engine::io::read_design_provenance_file(&path);
    apply(app, Some(path), report);
    true
}

/// Take up a finished check, if one has answered.
pub fn tick(app: &mut SolveApp) {
    let Some(verifying) = app.verifying.as_ref() else { return };
    let answer = match verifying.done.try_recv() {
        Ok(answer) => answer,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => None,
    };
    let Some(verifying) = app.verifying.take() else { return };
    apply(app, Some(verifying.path), answer);
}

/// Show what a check found, when it checked the design still in front.
fn apply(app: &mut SolveApp, path: Option<String>, report: Option<ProvenanceReport>) {
    let Some(path) = path else {
        app.set_status(solidcraft_ui_text::tr("ui.verify_stopped").to_string(), true);
        return;
    };
    if app.session.path.as_deref() != Some(path.as_str()) {
        return;
    }
    app.session.provenance = report.clone();
    match report {
        Some(report) => {
            let text = crate::documents::signature_line(&report);
            let invalid = matches!(report.status, solidcraft_engine::io::ProvenanceStatus::Invalid(_));
            app.set_status(text, invalid);
        }
        None => app.set_status(solidcraft_ui_text::tr("ui.verify_unsigned").to_string(), false),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn checking_an_unsigned_design_says_there_is_no_signature() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let path = h.dir.join("Unsigned.solidcraft");
        h.app.run("file.save_as", json!({"path": path.to_string_lossy()})).unwrap();
        assert!(super::start(&mut h.app), "a check starts on a background thread");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while h.app.verifying.is_some() && std::time::Instant::now() < deadline {
            super::tick(&mut h.app);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(h.app.verifying.is_none(), "the check answered");
        let (text, _, error) = h.app.status.as_ref().expect("the result is shown");
        assert!(text.contains("no signature"), "{text}");
        assert!(!error);
    }

    #[test]
    fn checking_a_design_with_no_file_says_to_save_it_first() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        assert!(!super::start(&mut h.app));
        let (text, _, error) = h.app.status.as_ref().expect("the reason is shown");
        assert!(text.contains("Save the design"), "{text}");
        assert!(error);
    }
}
