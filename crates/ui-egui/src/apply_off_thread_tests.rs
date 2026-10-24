//! OK off the window thread: a feature that takes longer than the app's wait is computed beside
//! the window, the dialog saying so, and applied once it is in; Cancel drops it.

use std::time::Duration;

use serde_json::json;

use crate::scenario::Harness;

/// A 60 × 20 × 10 block and, just finished on its top, a 10 × 10 square: Extrude takes it.
fn square_on_a_block() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [60, 20]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 10})),
        ("sketch.create", json!({"plane": {"origin": [0, 0, 10], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("sketch.rectangle.two_point", json!({"p0": [5, 5], "p1": [15, 15]})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.app.finish_sketch();
    h.app.start("solid.extrude");
    h.frames(2);
    h
}

/// Frames until OK's evaluation is in and applied (a few seconds at most).
fn until_applied(h: &mut Harness) {
    for _ in 0..500 {
        if h.app.applying.is_none() {
            break;
        }
        h.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    h.frames(1);
}

#[test]
fn ok_computes_off_the_window_thread_saying_so_then_applies() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    // The evaluation is held until the dialog has been seen saying so (it is in the cache all
    // the same), so no frame can apply it before the checks below.
    let (finish, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.applying.is_some() && h.app.dialog.is_some(), "computing, the dialog open");
    h.frames(1);
    assert!(h.widget("Computing… 0 s").is_some(), "the dialog says it computes");
    // Enter again does not start it twice.
    h.step(&json!({"key": "Enter"})).unwrap();
    finish.send(()).unwrap();
    until_applied(&mut h);
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    h.step(&json!({"expect": {"bodies": 1, "volume": 13000}})).unwrap();
    assert_eq!(h.app.session.doc.features.len(), 4, "one extrude added");
}

#[test]
fn cancel_while_computing_applies_nothing() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    // Held, as the preview's evaluation is in the cache and would otherwise be in before OK's
    // wait is over, applying at once.
    let (finish, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    let features = h.app.session.doc.features.len();
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.applying.is_some());
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(h.app.applying.is_none() && h.app.dialog.is_none());
    finish.send(()).unwrap();
    h.frames(5);
    assert_eq!(h.app.session.doc.features.len(), features);
}

/// #2929: the feature must not appear in the frame OK starts computing in. The window thread
/// can be held up between starting the evaluation and the check that takes it up, and the
/// feature then appears where the dialog says it is computing - which is what made
/// `cancel_while_computing_applies_nothing` fail under load.
#[test]
fn ok_computing_off_the_window_thread_does_not_apply_in_the_frame_it_starts() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    // Held past OK's wait (the evaluation is in the cache), then let go before the next frame.
    let (finish, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    let features = h.app.session.doc.features.len();
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.applying.is_some(), "the dialog is computing");
    finish.send(()).unwrap();
    assert_eq!(h.app.session.doc.features.len(), features, "nothing applied in the frame it started");
    // A frame later the evaluation is in and the feature is applied.
    until_applied(&mut h);
    assert_eq!(h.app.session.doc.features.len(), features + 1);
}

#[test]
fn the_dialog_holds_its_inputs_while_ok_computes() {
    let mut h = square_on_a_block();
    h.app.fit_view();
    h.frames(2);
    let picked = h.app.dialog.as_ref().map(|d| d.inputs[0].items.clone()).unwrap();
    // OK computing, until the test lets it finish.
    let (finish, done) = std::sync::mpsc::channel();
    let cmds = h.app.dialog.as_ref().and_then(|d| crate::dialogs::apply_commands(&h.app, d).ok()).unwrap();
    h.app.applying = Some(crate::dialogs::Applying::computing(done, cmds));
    // A click on the square picked would take it out of the profiles.
    h.step(&json!({"click": {"world": [10, 10, 10]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().map(|d| d.inputs[0].items.clone()), Some(picked), "no pick while computing");
    finish.send(None).unwrap();
    until_applied(&mut h);
    assert!(h.app.dialog.is_none());
    h.step(&json!({"expect": {"bodies": 1, "volume": 13000}})).unwrap();
}

/// Inputs that changed while OK computed (nothing should change them) are computed in turn, off
/// the window thread too, then applied: run as OK finished, they would hold the window for the
/// whole boolean.
#[test]
fn inputs_changed_while_ok_computes_are_computed_off_the_thread_too() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    let features = h.app.session.doc.features.len();
    // OK computing other commands than the dialog's, until the test lets it finish.
    let (finish, done) = std::sync::mpsc::channel();
    h.app.applying = Some(crate::dialogs::Applying::computing(done, Vec::new()));
    finish.send(None).unwrap();
    h.frames(1);
    assert!(h.app.applying.is_some(), "the dialog's own commands are computing");
    assert_eq!(h.app.session.doc.features.len(), features, "nothing applied on the window thread");
    until_applied(&mut h);
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.features.len(), features + 1);
    h.step(&json!({"expect": {"bodies": 1, "volume": 13000}})).unwrap();
}

/// #3962: OK pressed again after a Cancel left its evaluation computing takes that job up: the
/// evaluation runs once. The worker is held until it is let go once; a second worker started
/// beside the first would wait for a second release that never comes, and nothing would apply.
#[test]
fn ok_again_after_cancel_takes_up_the_evaluation_still_computing() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    let (finish, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    let features = h.app.session.doc.features.len();
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.applying.is_some(), "computing");
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(h.app.applying.is_none() && h.app.dialog.is_none());
    assert_eq!(h.app.abandoned.len(), 1, "left computing");
    h.app.start("solid.extrude");
    h.frames(2);
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.applying.is_some() && h.app.abandoned.is_empty(), "the same evaluation taken up");
    finish.send(()).unwrap();
    until_applied(&mut h);
    assert!(h.app.applying.is_none(), "applied once its one evaluation is in");
    assert_eq!(h.app.session.doc.features.len(), features + 1);
}

/// #3962: quitting does not wait for an evaluation a Cancel left computing.
#[test]
fn quitting_does_not_wait_for_an_evaluation_left_computing() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    // Held for the whole test: the evaluation never finishes.
    let (_finish, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    h.step(&json!({"key": "Enter"})).unwrap();
    h.step(&json!({"key": "Escape"})).unwrap();
    assert_eq!(h.app.abandoned.len(), 1);
    h.app.session.mark_saved();
    crate::documents::request_quit(&mut h.app);
    h.frames(2);
    assert!(h.app.quit_requested, "the window closes with the evaluation still running");
}
