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
    /// The file's size and modified time when the check started: a save that replaces the file
    /// while the check runs makes the report answer for a version that is no longer there.
    disk: Option<solidcraft_engine::io::design_lock::FileStamp>,
    done: mpsc::Receiver<Result<Option<ProvenanceReport>, String>>,
}

/// Start checking the active design's signature. `false` when nothing started (there is no file,
/// a save is being written, or a check is already running).
pub fn start(app: &mut SolveApp) -> bool {
    start_with(app, false)
}

/// Check after an already-open design was reused: the open request is what is finishing, so the
/// file-load guard does not apply.
pub fn start_after_reuse(app: &mut SolveApp) -> bool {
    start_with(app, true)
}

fn start_with(app: &mut SolveApp, after_reuse: bool) -> bool {
    if app.verifying.is_some() {
        return false;
    }
    if app.saving.is_some() || app.host_save.is_some() || app.file_picker.is_some() || (!after_reuse && app.file_load_holds()) {
        app.set_status(solidcraft_ui_text::tr("ui.verify_waits_for_file_operation").to_string(), true);
        return false;
    }
    let Some(path) = app.session.path.clone() else {
        app.set_status(solidcraft_ui_text::tr("ui.verify_needs_file").to_string(), true);
        return false;
    };
    let disk = solidcraft_engine::io::design_lock::stamp(&path);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = mpsc::channel();
        let checked = path.clone();
        let spawned = std::thread::Builder::new().name("verify".into()).spawn(move || {
            let report = solidcraft_engine::io::read_design_provenance_file_checked(&checked).map_err(|e| e.to_string());
            let _ = tx.send(report);
        });
        if spawned.is_ok() {
            app.verifying = Some(Verifying { path, disk, done: rx });
            app.set_status(solidcraft_ui_text::tr("ui.verify_checking").to_string(), false);
            return true;
        }
        // No thread to read on: check here, as the menu would have before there was a worker.
    }
    // The browser holds designs in memory (no threads): check here, which is quick.
    let report = solidcraft_engine::io::read_design_provenance_file_checked(&path).map_err(|e| e.to_string());
    apply(app, Some(path), disk, report);
    true
}

/// Take up a finished check, if one has answered.
pub fn tick(app: &mut SolveApp) {
    let Some(verifying) = app.verifying.as_ref() else { return };
    let answer = match verifying.done.try_recv() {
        Ok(answer) => answer,
        Err(mpsc::TryRecvError::Empty) => return,
        Err(mpsc::TryRecvError::Disconnected) => Err("the check stopped".to_string()),
    };
    let Some(verifying) = app.verifying.take() else { return };
    apply(app, Some(verifying.path), verifying.disk, answer);
}

/// Show what a check found, when it checked the design still in front and the file is unchanged.
fn apply(
    app: &mut SolveApp,
    path: Option<String>,
    disk: Option<solidcraft_engine::io::design_lock::FileStamp>,
    result: Result<Option<ProvenanceReport>, String>,
) {
    let Some(path) = path else {
        app.set_status(solidcraft_ui_text::tr("ui.verify_stopped").to_string(), true);
        return;
    };
    if app.session.path.as_deref() != Some(path.as_str()) {
        return;
    }
    if solidcraft_engine::io::design_lock::stamp(&path) != disk {
        app.set_status(solidcraft_ui_text::tr("ui.verify_file_changed").to_string(), true);
        return;
    }
    let report = match result {
        Ok(report) => report,
        Err(reason) => {
            app.set_status(solidcraft_ui_text::format("ui.verify_failed", &[("reason", &reason)]), true);
            return;
        }
    };
    app.session.provenance = report.clone();
    match report {
        Some(report) => {
            let trusted = report.newest().is_some_and(|record| crate::documents::signer_trusted(app, record));
            let text = crate::documents::signature_line(&report, trusted);
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
    fn a_missing_file_is_a_check_error_not_an_unsigned_design() {
        let mut h = crate::scenario::Harness::new();
        h.app.home.open = false;
        h.app.session.path = Some("/missing/solidcraft-review.sc".into());
        assert!(super::start(&mut h.app));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while h.app.verifying.is_some() && std::time::Instant::now() < deadline {
            super::tick(&mut h.app);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let (text, _, error) = h.app.status.as_ref().expect("the error is shown");
        assert!(text.contains("Could not check the signature"), "{text}");
        assert!(error);
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
