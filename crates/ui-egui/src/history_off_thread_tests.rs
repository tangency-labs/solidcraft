//! Commands that recompute the history off the window thread (`deferred`): the marker moved,
//! undo, each computed beside the window and run once in; a marker moved again while one move
//! computes replaces the move still waiting. And an agent's control requests (`control_job`,
//! #1989): its design changes computed beside the window, in order, while the window answers.

use std::time::Duration;

use serde_json::json;

use crate::scenario::Harness;

/// A 60 × 20 × 10 block, a 10 × 10 × 5 boss on it, and a 4 mm hole through the boss, the
/// marker at the end. Every command computes off the window thread (no wait at all).
fn block_boss_hole() -> Harness {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, p) in [
        ("solid.box", json!({"length": 60, "width": 20, "height": 10})),
        ("solid.box", json!({"corner": [5, 5, 10], "length": 10, "width": 10, "height": 5, "operation": "join"})),
        ("solid.cylinder", json!({"base": [10, 10, -1], "axis": [0, 0, 1], "radius": 2, "height": 20, "operation": "cut"})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.app.apply_off_thread = Some(Duration::ZERO);
    h
}

/// Frames until nothing computes or waits (a few seconds at most).
fn until_settled(h: &mut Harness) {
    for _ in 0..500 {
        if h.app.deferred.is_none() && h.app.deferred_queue.is_empty() && h.app.delete_check.is_none() && h.app.pending_feature_edit.is_none() {
            break;
        }
        h.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    h.frames(1);
}

fn marker(h: &Harness) -> Option<usize> {
    h.app.session.doc.marker
}

#[test]
fn the_marker_moved_computes_off_the_window_thread_then_moves() {
    let mut h = block_boss_hole();
    h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
    assert!(h.app.computing_since().is_some(), "computing");
    assert_eq!(marker(&h), None, "the design is as it was while it computes");
    until_settled(&mut h);
    assert_eq!(marker(&h), Some(1));
    h.step(&json!({"expect": {"volume": 12000}})).unwrap();
    // And back to the end, the boss and the hole recomputed (or found in the cache).
    h.app.run_heavy("timeline.roll_to", json!({})).unwrap();
    until_settled(&mut h);
    assert_eq!(marker(&h), None);
    let hole = std::f64::consts::PI * 4.0 * 15.0;
    let v = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((v - (12500.0 - hole)).abs() < 1.0, "{v}");
}

/// The command runs for real with what was evaluated off the window thread: nothing is
/// evaluated twice, even once the evaluation cache no longer holds it (a large design's states
/// are past its budget, and the window thread evaluated the whole command again).
#[test]
fn a_command_computed_off_the_thread_is_not_evaluated_again() {
    let mut h = block_boss_hole();
    h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
    until_settled(&mut h);
    h.app.session.model.cache = Default::default();
    h.app.run_heavy("timeline.roll_to", json!({})).unwrap();
    for _ in 0..500 {
        if h.app.deferred.as_mut().is_some_and(crate::deferred::Deferred::finished) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // What it evaluated is gone from the cache.
    h.app.session.model.cache = Default::default();
    until_settled(&mut h);
    assert_eq!(marker(&h), None);
    assert_eq!((h.app.session.model.last_recomputed, h.app.session.model.last_restored), (0, 2), "the boss and the hole as evaluated off the thread");
    let hole = std::f64::consts::PI * 4.0 * 15.0;
    let v = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((v - (12500.0 - hole)).abs() < 1.0, "{v}");
}

/// A parameter change computed off the window thread runs here without computing a feature
/// again, even with the evaluation cache emptied meanwhile (a design whose states pass its
/// budget): it evaluates once before its sketch stores the new solution and once after, and the
/// first of those is kept for the run here too, not only the last.
#[test]
fn a_parameter_change_computed_off_the_thread_is_not_evaluated_again() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, p) in [
        ("parameters.add", json!({"name": "W", "expression": "40 mm"})),
        ("sketch.create", json!({"plane": "XY", "name": "Base"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]})),
        ("sketch.dimension", json!({"entities": ["l1"], "value": "W"})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 10})),
        ("solid.box", json!({"corner": [5, 5, 10], "length": 10, "width": 10, "height": 5, "operation": "join"})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.app.apply_off_thread = Some(Duration::ZERO);
    h.app.run_heavy("parameters.change", json!({"name": "W", "expression": "50 mm"})).unwrap();
    for _ in 0..500 {
        if h.app.deferred.as_mut().is_some_and(crate::deferred::Deferred::finished) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    h.app.session.model.cache = Default::default();
    let before = h.app.session.model.recomputed;
    until_settled(&mut h);
    assert_eq!(h.app.session.model.recomputed - before, 0, "nothing computed again on the window thread, in either evaluation");
    let v = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((v - (50.0 * 20.0 * 10.0 + 500.0)).abs() < 1.0, "{v}");
}

#[test]
fn playback_waits_for_each_background_rebuild_before_timing_the_next_step() {
    let mut h = block_boss_hole();
    h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
    h.app.start("timeline.play");
    h.frames(1);
    for position in 1..=3 {
        assert!(h.app.deferred.is_some(), "step {position} is computed in the background");
        assert!(h.app.deferred_queue.is_empty(), "playback must not queue steps that supersede each other");
        for _ in 0..500 {
            if h.app.deferred.as_mut().is_some_and(crate::deferred::Deferred::finished) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // A large design can evict these states before the UI receives the completed step.
        // Playback must consume the worker's evaluated features, not rebuild them again.
        h.app.session.model.cache = Default::default();
        until_settled(&mut h);
        assert_eq!(h.app.session.model.last_recomputed, 0, "playback step {position} uses the background evaluation");
        assert_eq!(marker(&h).unwrap_or(3), position);
        h.frames(10);
        assert_eq!(marker(&h).unwrap_or(3), position, "step {position} stays visible after computation");
        if position < 3 {
            // Advance only until the next step starts; never sleep through a whole playback.
            for _ in 0..40 {
                if h.app.deferred.is_some() {
                    break;
                }
                h.frames(1);
            }
        }
    }
    assert!(h.app.timeline_playback.is_none());
    assert!(h.app.deferred_queue.is_empty());
}

#[test]
fn interrupted_playback_never_applies_its_pending_background_step() {
    for action in ["stop", "last", "edit", "marker", "tool", "palette", "new"] {
        let mut h = block_boss_hole();
        h.app.run("timeline.roll_to", json!({"position": 0})).unwrap();
        h.app.start("timeline.play");
        h.frames(1);
        assert!(h.app.deferred.is_some());
        match action {
            "stop" => h.app.start("timeline.play"),
            "last" => {
                crate::timeline::run_transport(&mut h.app, crate::ui_kit::HistoryTransport::Last);
                until_settled(&mut h);
                assert_eq!(marker(&h), None, "Go to end supersedes the pending playback step");
            }
            "edit" => {
                h.app.run("timeline.rename", json!({"feature": 1, "name": "Renamed"})).unwrap();
            }
            "marker" => {
                h.app.run("timeline.roll_to", json!({"position": 2})).unwrap();
            }
            "tool" => h.app.start("solid.box"),
            "palette" => {
                h.app.start("ui.palette");
                assert!(h.app.timeline_playback.is_some());
                h.app.start("timeline.play");
            }
            _ => {
                h.app.run("file.new", json!({})).unwrap();
            }
        }
        assert!(h.app.timeline_playback.is_none(), "{action}");
        assert!(h.app.deferred.is_none(), "{action}: the worker cannot apply the old marker");
        let marker = marker(&h);
        h.frames(90);
        assert_eq!(h.app.session.doc.marker, marker, "{action}");
    }
}

#[test]
fn a_marker_moved_again_replaces_the_move_still_waiting() {
    let mut h = block_boss_hole();
    h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
    assert!(h.app.computing_since().is_some());
    // A drag: two more slots while the first computes. Only the last one waits.
    h.app.run_heavy("timeline.roll_to", json!({"position": 2})).unwrap();
    h.app.run_heavy("timeline.roll_to", json!({"position": 3})).unwrap();
    assert_eq!(h.app.deferred_queue.len(), 1, "{:?}", h.app.deferred_queue);
    until_settled(&mut h);
    assert_eq!(marker(&h), None, "where the drag ended: the end of the history");
}

/// The "Computing" toast has a Cancel (#8266): it gives up on a marker move that takes too long, and
/// on the moves waiting behind it, and the design stays as it was.
#[test]
fn cancel_on_the_computing_toast_gives_up_on_the_move() {
    let mut h = block_boss_hole();
    // A move that does not finish, with another waiting behind it.
    let (job, _keep) = crate::deferred::Deferred::never_done("timeline.roll_to", json!({"position": 1}), h.app.session.revision);
    h.app.deferred = Some(job);
    h.app.deferred_queue.push(("timeline.roll_to".into(), json!({"position": 2})));
    h.frames(2);
    let toast = crate::scenario::rect_of("toast:computing").expect("the computing toast");
    let cancel = crate::scenario::handle_at("toast:computing:cancel").expect("its Cancel");
    assert!(toast.contains(cancel), "{cancel:?} in {toast:?}");
    assert!(h.widget("Cancel").is_some(), "Cancel has an accessible name");
    h.step(&json!({"click": {"handle": "toast:computing:cancel"}})).unwrap();
    h.frames(2);
    assert!(h.app.deferred.is_none() && h.app.deferred_queue.is_empty(), "nothing computes or waits");
    assert!(h.app.computing_since().is_none());
    assert!(crate::scenario::rect_of("toast:computing").is_none(), "the toast is gone");
    assert_eq!(marker(&h), None, "the marker stays where it was");
    // And the history moves again afterwards.
    h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
    until_settled(&mut h);
    assert_eq!(marker(&h), Some(1));
}

/// A feature edit applying has the dialog's own Cancel, so its toast has none.
#[test]
fn the_computing_toast_has_no_cancel_for_what_cannot_be_given_up() {
    let mut h = block_boss_hole();
    assert!(!h.app.can_cancel_computing(), "nothing computes");
    h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
    assert!(h.app.can_cancel_computing(), "a move can be given up");
    until_settled(&mut h);
    assert!(!h.app.can_cancel_computing());
}

#[test]
fn undo_computes_off_the_window_thread() {
    let mut h = block_boss_hole();
    let features = h.app.session.doc.features.len();
    h.app.start("edit.undo");
    assert!(h.app.computing_since().is_some(), "undo computing");
    assert_eq!(h.app.session.doc.features.len(), features, "nothing undone yet");
    until_settled(&mut h);
    assert_eq!(h.app.session.doc.features.len(), features - 1, "the hole undone");
    h.step(&json!({"expect": {"volume": 12500}})).unwrap();
    h.app.start("edit.redo");
    until_settled(&mut h);
    assert_eq!(h.app.session.doc.features.len(), features, "and redone");
}

#[test]
fn a_design_changed_while_a_move_computes_is_computed_again() {
    let mut h = block_boss_hole();
    h.app.run_heavy("timeline.roll_to", json!({"position": 2})).unwrap();
    assert!(h.app.computing_since().is_some());
    // Something else changes the design meanwhile (a command that just runs).
    h.app.run("solid.box", json!({"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"})).unwrap();
    let revision = h.app.session.revision;
    until_settled(&mut h);
    assert!(h.app.session.revision > revision, "the move ran after the change");
    assert_eq!(marker(&h), Some(2));
}

/// Frames of a queued gesture until a history change starts computing, then that computation
/// is held: it finishes when the returned sender is dropped, so the frames between see the
/// history busy for as long as a test needs.
fn hold_the_change(h: &mut Harness) -> std::sync::mpsc::Sender<Option<solidcraft_engine::Evaluated>> {
    for _ in 0..100 {
        if h.app.deferred.is_some() {
            break;
        }
        h.frames(1);
    }
    let job = h.app.deferred.as_mut();
    assert!(job.is_some(), "a history change is computing");
    job.map(crate::deferred::Deferred::hold).unwrap()
}

fn names(h: &Harness) -> Vec<String> {
    h.app.session.doc.features.iter().map(|f| f.name.clone()).collect()
}

/// While a dropped marker's move computes, the bottom strip shows the marker where it was let
/// go and looks busy, and neither another drag nor a transport click queues a change; the move
/// then lands where it was dropped.
#[test]
fn the_bottom_strip_shows_a_dropped_move_and_takes_nothing_more_while_it_computes() {
    use crate::scenario::{handle_at, rect_of};
    let mut h = block_boss_hole();
    h.frames(3);
    let features = names(&h);
    let at = |name: &str| handle_at(&format!("timeline:{name}")).unwrap();
    let (first, second) = (at(&features[0]), at(&features[1]));
    let from = rect_of("history:marker").unwrap().center();
    h.queue_drag(from, egui::pos2(second.x - 16.0, second.y), 8, 0);
    let done = hold_the_change(&mut h);
    h.frames(3);
    assert_eq!(marker(&h), None, "the design is as it was while the move computes");
    let shown = rect_of("history:marker").unwrap().center();
    assert!(at(&features[0]).x < shown.x && shown.x < at(&features[1]).x, "the marker shows where it was dropped: {shown:?}");
    assert!(rect_of("history:busy").is_some(), "the strip looks busy");
    h.step(&json!({"drag": [[shown.x, shown.y], [first.x - 16.0, first.y]]})).unwrap();
    h.step(&json!({"click": {"handle": "history:last"}})).unwrap();
    assert!(h.app.deferred_queue.is_empty(), "nothing queued behind the move: {:?}", h.app.deferred_queue);
    drop(done);
    until_settled(&mut h);
    assert_eq!(marker(&h), Some(1), "the move landed where it was dropped");
    // Once it has landed the dropped row is forgotten: while a later change computes, the strip
    // shows the design as it is, not that drop.
    h.step(&json!({"click": {"handle": "history:last"}})).unwrap();
    until_settled(&mut h);
    let end = rect_of("history:marker").unwrap().center();
    crate::timeline::run_transport(&mut h.app, crate::ui_kit::HistoryTransport::First);
    let done = hold_the_change(&mut h);
    h.frames(2);
    assert_eq!(rect_of("history:marker").unwrap().center(), end, "the marker as the design has it, at the end");
    drop(done);
    until_settled(&mut h);
}

/// The view's "Computing…" toast is left out while History shows its own busy look, and still
/// shown when History is folded away.
#[test]
fn the_view_says_computing_only_when_history_does_not() {
    use crate::scenario::rect_of;
    for folded in [false, true] {
        let mut h = block_boss_hole();
        if folded {
            h.app.preferences.timeline_left = true;
            h.app.preferences.timeline_under_browser = true;
            h.app.preferences.timeline_collapsed = true;
        }
        h.frames(3);
        h.app.run_heavy("timeline.roll_to", json!({"position": 1})).unwrap();
        let done = hold_the_change(&mut h);
        h.frames(3);
        assert_eq!(rect_of("history:busy").is_some(), !folded, "folded={folded}: History looks busy");
        assert_eq!(rect_of("toast:computing").is_some(), folded, "folded={folded}: the view's toast");
        drop(done);
        until_settled(&mut h);
    }
}

/// Reordering in the History beside the Browser recomputes off the window thread, as the bottom
/// strip's does: the window keeps drawing while the design recomputes.
#[test]
fn a_reorder_in_the_history_beside_the_browser_computes_off_the_window_thread() {
    use crate::scenario::handle_at;
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.preferences.timeline_left = true;
    for (k, name) in ["A", "B", "C"].into_iter().enumerate() {
        let corner = [k as f64 * 20.0, 0.0, 0.0];
        h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": corner, "operation": "new_body", "name": name})).unwrap();
    }
    h.app.apply_off_thread = Some(Duration::ZERO);
    h.frames(3);
    let (a, c) = (handle_at("timeline:A").unwrap(), handle_at("timeline:C").unwrap());
    h.queue_drag(c, egui::pos2(a.x, a.y - 8.0), 8, 0);
    for _ in 0..100 {
        if h.app.deferred.is_some() || h.app.synthetic.is_empty() {
            break;
        }
        h.frames(1);
    }
    assert!(h.app.computing_since().is_some(), "the reorder computes off the window thread");
    assert_eq!(names(&h), ["A", "B", "C"], "nothing moved yet");
    until_settled(&mut h);
    assert_eq!(names(&h), ["C", "A", "B"]);
}

/// A click on the history marker, without moving it, rolls nothing back or forward: no
/// recompute is asked for and nothing is added to the undo history, in either layout.
#[test]
fn a_click_on_the_history_marker_asks_for_nothing() {
    use crate::scenario::rect_of;
    for left in [false, true] {
        let mut h = block_boss_hole();
        h.app.preferences.timeline_left = left;
        h.frames(3);
        let undo = h.app.session.undo.len();
        let at = rect_of("history:marker").unwrap().center();
        h.step(&json!({"click": [at.x, at.y]})).unwrap();
        h.frames(2);
        assert!(h.app.deferred.is_none() && h.app.deferred_queue.is_empty(), "left={left}: nothing computing");
        assert_eq!((h.app.session.undo.len(), marker(&h)), (undo, None), "left={left}");
    }
}

fn sketches_with_later_bodies() -> (Harness, Vec<u64>) {
    let mut h = Harness::new();
    h.app.home.open = false;
    let mut sketches = Vec::new();
    for x in [0, 20] {
        let sketch = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
        sketches.push(sketch);
        h.app.run("sketch.line", json!({"points": [[x, 0], [x + 10, 0]]})).unwrap();
        h.app.finish_sketch();
        h.app.run("solid.box", json!({"corner": [x, 0, 0], "length": 10, "width": 10, "height": 10})).unwrap();
    }
    h.app.run("solid.box", json!({"corner": [40, 0, 0], "length": 10, "width": 10, "height": 10})).unwrap();
    h.app.apply_off_thread = Some(Duration::ZERO);
    (h, sketches)
}

/// #5435: switching editors must not leave a background restore from the first sketch that
/// moves the second editor's marker, or make Finish restore to the first sketch's rollback.
#[test]
fn switching_sketches_keeps_the_original_marker_with_background_rebuilds() {
    for original_marker in [None, Some(4)] {
        for cancel in [false, true] {
            let (mut h, sketches) = sketches_with_later_bodies();
            h.app.run("timeline.roll_to", json!({"position": original_marker})).unwrap();
            let bodies = h.app.session.model.state().bodies.len();
            h.app.edit_sketch(sketches[0]);
            h.app.edit_sketch(sketches[1]);
            until_settled(&mut h);
            assert_eq!(h.app.session.active_sketch, Some(sketches[1]));
            assert_eq!(marker(&h), Some(3), "stay at the sketch being edited");
            if cancel {
                h.app.cancel_sketch();
            } else {
                h.app.finish_sketch();
            }
            until_settled(&mut h);
            assert_eq!(marker(&h), original_marker, "restore the marker from before either edit");
            assert_eq!(h.app.session.model.state().bodies.len(), bodies, "all later bodies return");
        }
    }
}

#[test]
fn switching_from_a_sketch_to_a_feature_keeps_the_original_marker_with_background_rebuilds() {
    let (mut h, sketches) = sketches_with_later_bodies();
    let feature = h.app.session.doc.features[3].id;
    let bodies = h.app.session.model.state().bodies.len();
    h.app.edit_sketch(sketches[0]);
    h.app.edit_feature(feature);
    until_settled(&mut h);
    assert_eq!(h.app.session.active_sketch, None);
    assert_eq!(marker(&h), Some(3), "stay just before the feature being edited");
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.editing), Some((feature, None)));
    crate::dialogs::cancel(&mut h.app);
    until_settled(&mut h);
    assert_eq!(marker(&h), None);
    assert_eq!(h.app.session.model.state().bodies.len(), bodies, "all later bodies return");
}

#[test]
fn switching_from_a_sketch_to_a_feature_restores_partial_markers_and_sketch_changes() {
    for original_marker in [None, Some(4)] {
        let (mut h, sketches) = sketches_with_later_bodies();
        h.app.run("timeline.roll_to", json!({"position": original_marker})).unwrap();
        let bodies = h.app.session.model.state().bodies.len();
        let feature = h.app.session.doc.features[3].id;
        h.app.edit_sketch(sketches[0]);
        h.app.run("sketch.line", json!({"points": [[0, 2], [10, 2]]})).unwrap();
        h.app.edit_feature(feature);
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        let changed = h.app.session.doc.sketch(sketches[0]).unwrap().clone();
        until_settled(&mut h);
        assert!(h.app.dialog.is_some());
        h.app.session.selection = vec![solidcraft_engine::Sel::Axis { name: "X".into() }];
        *h.app.session.model.cache.lock().unwrap() = Default::default();
        let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
        solidcraft_engine::doc::progress::watch(window.clone(), || crate::dialogs::cancel(&mut h.app));
        assert_eq!(window.features(), (0, 0), "Cancel uses the pinned matching model");
        assert_eq!(marker(&h), original_marker);
        assert_eq!(h.app.session.model.state().bodies.len(), bodies);
        assert_eq!(h.app.session.doc.sketch(sketches[0]).unwrap(), &changed);
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Axis { name: "X".into() }]);
    }
}

#[test]
fn switching_from_a_sketch_to_a_feature_rejects_replaced_or_changed_documents() {
    for action in ["new", "change"] {
        let (mut h, sketches) = sketches_with_later_bodies();
        let feature = h.app.session.doc.features[3].id;
        h.app.edit_sketch(sketches[0]);
        h.app.edit_feature(feature);
        assert!(h.app.computing_since().is_some());
        match action {
            "new" => {
                h.app.run("file.new", json!({})).unwrap();
            }
            "change" => {
                h.app.run("solid.box", json!({"corner": [80, 0, 0], "length": 2, "width": 2, "height": 2})).unwrap();
            }
            _ => assert_eq!(action, "change", "unexpected test action"),
        }
        let current = h.app.session.doc.clone();
        let sketch = h.app.session.active_sketch;
        until_settled(&mut h);
        assert!(h.app.dialog.is_none(), "{action} must not open the obsolete editor");
        assert!(std::sync::Arc::ptr_eq(&current, &h.app.session.doc), "{action} must preserve the current document");
        assert_eq!(h.app.session.active_sketch, sketch);
    }
}

#[test]
fn switching_from_a_sketch_to_a_feature_applies_with_one_undo_and_redo() {
    let (mut h, sketches) = sketches_with_later_bodies();
    let feature = h.app.session.doc.features[3].id;
    let before = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    h.app.edit_sketch(sketches[0]);
    h.app.edit_feature(feature);
    let undo = h.app.session.undo.len();
    until_settled(&mut h);
    h.frames(2);
    h.step(&json!({"click":{"handle":"dialog:ui.length"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"12 mm"})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
    for _ in 0..500 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.app.dialog.is_none(), "the edit must finish");
    assert_eq!(marker(&h), None);
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let after = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((after - before - 200.0).abs() < 0.001);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(marker(&h), None);
    let restored = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((restored - before).abs() < 0.001);
    h.app.run("edit.redo", json!({})).unwrap();
    let redone = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((redone - after).abs() < 0.001);
}

#[test]
fn switching_from_a_sketch_to_a_feature_preserves_the_marker_through_cancel_and_superseding_editors() {
    for next in ["cancel", "escape", "sketch", "feature", "tab", "tab_back_pending"] {
        let (mut h, sketches) = sketches_with_later_bodies();
        let feature = h.app.session.doc.features[3].id;
        h.app.edit_sketch(sketches[0]);
        h.app.edit_feature(feature);
        match next {
            "cancel" => crate::dialogs::cancel(&mut h.app),
            "escape" => {
                h.step(&json!({"key":"Escape"})).unwrap();
            }
            "sketch" => {
                h.app.edit_sketch(sketches[1]);
                h.app.finish_sketch();
            }
            "feature" => {
                h.app.edit_feature(feature);
                until_settled(&mut h);
                crate::dialogs::cancel(&mut h.app);
            }
            _ => {
                let other = solidcraft_engine::Session::new(Default::default());
                crate::documents::open_tab(&mut h.app, other).unwrap();
                if next == "tab" {
                    h.frames(10);
                }
                assert!(h.app.session.doc.features.is_empty());
                crate::documents::switch(&mut h.app, 0);
            }
        }
        until_settled(&mut h);
        assert!(h.app.dialog.is_none(), "{next} must leave no feature editor");
        assert_eq!(marker(&h), None, "{next} must restore the original marker");
        assert_eq!(h.app.session.model.state().bodies.len(), 3);
    }
}

#[test]
fn switching_from_a_sketch_to_a_feature_defers_a_new_command_until_restoration() {
    let (mut h, sketches) = sketches_with_later_bodies();
    let feature = h.app.session.doc.features[3].id;
    let count = h.app.session.doc.features.len();
    h.app.edit_sketch(sketches[0]);
    h.app.edit_feature(feature);
    h.app.start("solid.box");
    assert!(h.app.dialog.is_none(), "the next dialog waits for the matching model");
    until_settled(&mut h);
    assert_eq!(marker(&h), None);
    assert_eq!(h.app.dialog.as_ref().unwrap().editing, None);
    let undo = h.app.session.undo.len();
    h.frames(2);
    h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
    for _ in 0..500 {
        if h.app.dialog.is_none() {
            break;
        }
        h.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(h.app.dialog.is_none());
    assert_eq!(marker(&h), None);
    assert_eq!(h.app.session.doc.features.len(), count + 1);
    assert_eq!(h.app.session.undo.len(), undo + 1);
}

#[test]
fn switching_from_a_sketch_to_a_feature_defers_press_pull_with_its_pick_contract() {
    let (mut h, sketches) = sketches_with_later_bodies();
    let feature = h.app.session.doc.features[3].id;
    h.app.edit_sketch(sketches[0]);
    h.app.edit_feature(feature);
    h.app.session.selection.clear();
    h.app.start("solid.press_pull");
    assert!(h.app.dialog.is_none());
    until_settled(&mut h);
    assert_eq!(marker(&h), None);
    let dialog = h.app.dialog.as_ref().unwrap();
    assert!(dialog.press_pull);
    let picks = dialog.inputs[0].accept;
    assert_ne!(picks & crate::selection::FACES, 0);
    assert_ne!(picks & crate::selection::EDGES, 0);
}

/// A control request for the app, and where its reply lands.
fn ask(tx: &std::sync::mpsc::Sender<crate::ControlRequest>, method: &str, params: serde_json::Value) -> std::sync::mpsc::Receiver<serde_json::Value> {
    let (req, reply) = crate::ControlRequest::new(method, params);
    tx.send(req).unwrap();
    reply
}

/// The block with its boss and hole, driven over the control channel.
fn agent_harness() -> (Harness, std::sync::mpsc::Sender<crate::ControlRequest>) {
    let mut h = block_boss_hole();
    h.app.preferences.agent_off_thread = true;
    let (tx, rx) = std::sync::mpsc::channel();
    h.app.control_rx = Some(rx);
    (h, tx)
}

/// Frames until `reply` lands (a few seconds at most).
fn reply_to(h: &mut Harness, reply: &std::sync::mpsc::Receiver<serde_json::Value>) -> serde_json::Value {
    for _ in 0..500 {
        if let Ok(v) = reply.try_recv() {
            return v;
        }
        h.frames(1);
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("no reply");
}

fn second_hole() -> serde_json::Value {
    json!({"command": "solid.cylinder", "params": {"base": [50, 10, -1], "axis": [0, 0, 1], "radius": 3, "height": 20, "operation": "cut"}})
}

/// While an agent's batch computes, the chip says which step it is on, of how many, and that
/// step's command (`2/3 · Cylinder`); a single command shows just its name.
#[test]
fn a_computing_batch_says_its_step() {
    let (mut h, tx) = agent_harness();
    let hole = |x: f64| json!({"command": "solid.cylinder", "params": {"base": [x, 10, -1], "axis": [0, 0, 1], "radius": 3, "height": 20, "operation": "cut"}});
    let batch = ask(&tx, "engine.batch", json!({"commands": [hole(50.0), hole(30.0), hole(70.0)]}));
    h.frames(1);
    let (_, label) = h.app.agent_computing().expect("the batch computes");
    let (step, command) = label.split_once(" · ").expect(&label);
    assert!(["1/3", "2/3", "3/3"].contains(&step), "{label}");
    assert!(!command.is_empty() && !command.contains('.'), "the toolbar's name, not the id: {label}");
    // Centred on the navigation bar and above it, clear of the window's own toasts.
    let chip = crate::scenario::rect_of("agent:chip").expect("the chip is drawn");
    let nav = h.app.viewport.layout.as_ref().expect("laid out").navigation;
    assert!((chip.center().x - nav.center().x).abs() < 1.0 && chip.bottom() < nav.top(), "{chip:?} not centred above {nav:?}");
    for toast in ["toast", "toast:computing"].into_iter().filter_map(crate::scenario::rect_of) {
        assert!(!chip.intersects(toast), "{chip:?} over {toast:?}");
    }
    assert_eq!(reply_to(&mut h, &batch)["ok"], true);
    let single = ask(&tx, "engine.execute", second_hole());
    h.frames(1);
    if let Some((_, label)) = h.app.agent_computing() {
        assert!(!label.contains('/'), "{label}");
    }
    assert_eq!(reply_to(&mut h, &single)["ok"], true);
}

/// #1989: an agent's design change computes off the window thread. The window keeps answering
/// (`ui.*`, inspection) while it computes and shows which command it is; the agent's next
/// design change waits its turn; each lands as its own undo step with its usual reply.
#[test]
fn an_agent_design_change_computes_off_the_window_thread() {
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let undo = h.app.session.undo.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    let view = ask(&tx, "ui.inspect", json!({}));
    let inspect = ask(&tx, "document.inspect", json!({}));
    let boss = ask(
        &tx,
        "engine.execute",
        json!({"command": "solid.box", "params": {"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"}}),
    );
    h.frames(1);
    assert!(h.app.agent_computing().is_some_and(|(_, label)| !label.is_empty()), "the window says the agent's command computes");
    assert!(view.try_recv().is_ok_and(|v| v["ok"] == true), "ui.* answered while it computes");
    assert!(boss.try_recv().is_err(), "the next design change waits its turn");
    assert_eq!(h.app.session.doc.features.len(), features, "nothing applied yet");
    // The inspection reads a copy of the design as it was when asked, off the window thread.
    let seen = reply_to(&mut h, &inspect);
    assert_eq!(seen["result"]["timeline"].as_array().map(Vec::len), Some(features), "the design as it was: {seen}");
    let r = reply_to(&mut h, &hole);
    assert_eq!(r["ok"], true, "{r}");
    let r = reply_to(&mut h, &boss);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.session.doc.features.len(), features + 2);
    assert_eq!(h.app.session.undo.len(), undo + 2, "one undo step each");
    assert!(h.app.agent_computing().is_none());
    // In the order they were asked for: undo takes the boss, the hole stays.
    h.app.run("edit.undo", json!({})).unwrap();
    let holes = std::f64::consts::PI * (4.0 * 15.0 + 9.0 * 10.0);
    let v = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((v - (12500.0 - holes)).abs() < 1.0, "{v}");
}

/// A person's edit while the agent's command computes goes first; the agent's command is
/// computed again for the design as it is and lands after it, never in between.
#[test]
fn a_persons_edit_meanwhile_goes_first_and_the_agents_lands_after() {
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    h.frames(1);
    assert!(h.app.agent_computing().is_some());
    h.app.run("solid.box", json!({"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"})).unwrap();
    let r = reply_to(&mut h, &hole);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.session.doc.features.len(), features + 2, "both kept");
    // Undo takes the agent's hole first: it came last.
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.features.len(), features + 1);
    let boss = 12500.0 + 125.0 - std::f64::consts::PI * 4.0 * 15.0;
    let v = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((v - boss).abs() < 1.0, "the person's boss stays: {v}");
}

/// A design replaced while the agent's command computes (another tab, a new design) is not
/// where it lands: it is refused, saying so, and nothing is applied.
#[test]
fn an_agent_command_is_refused_when_its_design_was_replaced() {
    let (mut h, tx) = agent_harness();
    let hole = ask(&tx, "engine.execute", second_hole());
    h.frames(1);
    assert!(h.app.agent_computing().is_some());
    h.app.run("file.new", json!({})).unwrap();
    let r = reply_to(&mut h, &hole);
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().is_some_and(|e| e.contains("replaced") && e.contains("nothing was applied")), "{r}");
    assert!(h.app.session.doc.features.is_empty(), "nothing went into the new design");
}

/// Stop answers the agent at once, applies nothing, and lets the requests behind it go on.
#[test]
fn stopping_an_agent_command_applies_nothing_and_goes_on() {
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    let boss = ask(
        &tx,
        "engine.execute",
        json!({"command": "solid.box", "params": {"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"}}),
    );
    h.frames(1);
    assert!(h.app.agent_computing().is_some());
    h.app.stop_agent_command();
    let r = hole.try_recv().expect("answered at once");
    assert!(r["ok"] == false && r["stopped"] == true, "{r}");
    let r = reply_to(&mut h, &boss);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.session.doc.features.len(), features + 1, "only the boss");
}

/// A batch computes off the window thread too, with the reply a batch run here gives.
#[test]
fn an_agent_batch_computes_off_the_window_thread() {
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let steps = json!({"commands": [second_hole(), {"command": "solid.box", "params": {"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"}}]});
    let batch = ask(&tx, "engine.batch", steps);
    h.frames(1);
    assert!(h.app.agent_computing().is_some(), "computing");
    assert_eq!(h.app.session.doc.features.len(), features);
    let r = reply_to(&mut h, &batch);
    assert_eq!((&r["ok"], &r["result"]["completed"], &r["result"]["undone"]), (&json!(true), &json!(2), &json!(0)), "{r}");
    assert_eq!(h.app.session.doc.features.len(), features + 2);
}

/// An export changes nothing in the design: it runs on a copy and answers from there, the file
/// written; a design change asked for meanwhile does not wait for it.
#[test]
fn an_agent_export_runs_on_a_copy() {
    let (mut h, tx) = agent_harness();
    let dir = std::env::temp_dir().join(format!("sc-agent-export-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("part.stl").to_string_lossy().to_string();
    let export = ask(&tx, "engine.execute", json!({"command": "file.export", "params": {"path": path}}));
    let r = reply_to(&mut h, &export);
    assert_eq!(r["ok"], true, "{r}");
    assert!(std::fs::metadata(&path).is_ok_and(|m| m.len() > 0), "written");
    assert!(h.app.session.log.iter().any(|l| l.contains("Export")), "echoed as when it runs here: {:?}", h.app.session.log);
    let _ = std::fs::remove_dir_all(&dir);
}

/// #4604: measuring the whole design (an inspection measures every body) reads a copy off the
/// window thread, preference or not: on a large assembly it took a minute, the window answered
/// nothing meanwhile and was closed as not responding. The window answers `ui.*` while it
/// measures, and the measure comes back as when it runs here.
#[test]
fn a_measure_reads_a_copy_off_the_window_thread_preference_or_not() {
    let (mut h, tx) = agent_harness();
    h.app.preferences.agent_off_thread = false;
    let here = h.app.run("inspect.measure", json!({})).unwrap();
    let log = h.app.session.log.len();
    let measure = ask(&tx, "engine.execute", json!({"command": "inspect.measure", "params": {}}));
    let inspect = ask(&tx, "document.inspect", json!({"measure": true}));
    let command = ask(&tx, "command", json!({"command": "document.inspect", "params": {"measure": true}}));
    let view = ask(&tx, "ui.inspect", json!({}));
    h.frames(1);
    assert!(view.try_recv().is_ok_and(|v| v["ok"] == true), "ui.* answered meanwhile");
    assert_eq!(h.app.control_answering.len(), 3, "all three read a copy");
    let r = reply_to(&mut h, &measure);
    assert_eq!(r["result"]["total"], here["total"], "{r}");
    for reply in [inspect, command] {
        let r = reply_to(&mut h, &reply);
        assert_eq!(r["ok"], true, "{r}");
        assert!(r["result"]["timeline"].is_array(), "{r}");
    }
    assert!(h.app.control_answering.is_empty());
    assert_eq!(h.app.session.log.len(), log, "a reading leaves no echo: {:?}", h.app.session.log);
}

/// #9624: an inspection read on a copy reports the design's undo and redo counts and its file,
/// as one run here does; the copy has no history and no file, and it said undo 0, redo 0, path
/// none, so an agent could not tell how far to roll back a failed batch.
#[test]
fn an_inspection_on_a_copy_reports_the_history_and_the_file() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.apply_off_thread = Some(Duration::ZERO);
    let (tx, rx) = std::sync::mpsc::channel();
    h.app.control_rx = Some(rx);
    let inspect = |h: &mut Harness| {
        let reply = ask(&tx, "document.inspect", json!({}));
        h.frames(1);
        assert_eq!(h.app.control_answering.len(), 1, "read a copy");
        let r = reply_to(h, &reply);
        assert_eq!(r["ok"], true, "{r}");
        let here = h.app.run("document.inspect", json!({})).unwrap();
        for key in ["undo", "redo", "path"] {
            assert_eq!(r["result"][key], here[key], "{key}: {r}");
        }
        (r["result"]["undo"].clone(), r["result"]["redo"].clone(), r["result"]["path"].clone())
    };
    for corner in [[0, 0, 0], [20, 0, 0]] {
        h.app.run("solid.box", json!({"corner": corner, "length": 10, "width": 10, "height": 10})).unwrap();
    }
    assert_eq!(inspect(&mut h), (json!(2), json!(0), json!(null)));
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(inspect(&mut h), (json!(1), json!(1), json!(null)));
    let path = h.dir.join("boxes.solidcraft");
    h.app.run("file.save_as", json!({"path": path})).unwrap();
    let (undo, redo, saved) = inspect(&mut h);
    assert_eq!((undo, redo), (json!(1), json!(1)));
    assert_eq!(saved, json!(h.app.session.path), "the file it was saved to");
    assert!(saved.as_str().is_some_and(|p| p.ends_with("boxes.solidcraft")), "{saved}");
}

/// A reading on a copy keeps the request's own `param_check` (#9184), as one run here does: its
/// reply says the level, and a word that is no level is refused.
#[test]
fn a_reading_on_a_copy_keeps_the_requests_param_check() {
    let (mut h, tx) = agent_harness();
    let measure = ask(&tx, "engine.execute", json!({"command": "inspect.measure", "params": {}, "param_check": "strict"}));
    h.frames(1);
    assert_eq!(h.app.control_answering.len(), 1, "read a copy");
    let r = reply_to(&mut h, &measure);
    assert_eq!((&r["ok"], &r["param_check"]), (&json!(true), &json!("strict")), "{r}");
    let loud = ask(&tx, "engine.execute", json!({"command": "inspect.measure", "params": {}, "param_check": "loud"}));
    let r = reply_to(&mut h, &loud);
    assert!(r["ok"] == false && r["error"].as_str().is_some_and(|e| e.contains("`param_check` must be off, warn or strict")), "{r}");
    // A design change computed ahead off the window thread runs here at the request's level.
    let features = h.app.session.doc.features.len();
    let misspelt = |level: &str| {
        let mut call = second_hole();
        call["params"]["operaton"] = json!("cut");
        call["param_check"] = json!(level);
        call
    };
    let strict = ask(&tx, "engine.execute", misspelt("strict"));
    h.frames(1);
    assert!(h.app.agent_computing().is_some(), "computed ahead off the window thread");
    let r = reply_to(&mut h, &strict);
    assert_eq!((&r["ok"], &r["param_check"]), (&json!(false), &json!("strict")), "{r}");
    assert_eq!(h.app.session.doc.features.len(), features, "refused unrun");
    let warn = ask(&tx, "engine.execute", misspelt("warn"));
    let r = reply_to(&mut h, &warn);
    assert_eq!(
        (&r["param_check"], &r["result"]["param_warnings"]),
        (&json!("warn"), &json!(["unknown key `operaton`; did you mean `operation`?"])),
        "{r}"
    );
    assert_eq!(h.app.session.param_check, solidcraft_engine::schema::ParamCheck::Off, "the app's own level again");
}

/// A clearance or interference check of every pair of bodies reads a copy off the window thread
/// too: on an assembly of 150 bodies it computed for minutes with the window not responding. The
/// answers are the ones running it here gives.
#[test]
fn a_clearance_check_reads_a_copy_off_the_window_thread() {
    let (mut h, tx) = agent_harness();
    h.app.preferences.agent_off_thread = false;
    // The answers but their timing.
    let untimed = |mut v: serde_json::Value| {
        if let Some(o) = v.as_object_mut() {
            o.remove("ms");
        }
        v
    };
    let clearance_here = untimed(h.app.run("inspect.clearance", json!({})).unwrap());
    let interference_here = untimed(h.app.run("inspect.interference", json!({})).unwrap());
    let clearance = ask(&tx, "engine.execute", json!({"command": "inspect.clearance", "params": {}}));
    let interference = ask(&tx, "engine.execute", json!({"command": "inspect.interference", "params": {}}));
    let view = ask(&tx, "ui.inspect", json!({}));
    h.frames(1);
    assert!(view.try_recv().is_ok_and(|v| v["ok"] == true), "ui.* answered meanwhile");
    assert_eq!(h.app.control_answering.len(), 2, "both read a copy");
    let r = reply_to(&mut h, &clearance);
    assert_eq!(untimed(r["result"].clone()), clearance_here, "{r}");
    let r = reply_to(&mut h, &interference);
    assert_eq!(untimed(r["result"].clone()), interference_here, "{r}");
    assert!(h.app.control_answering.is_empty());
}

/// With the preference off (Preferences > AI, the default) an agent's design change runs on the
/// window thread, as before: answered in the frame it arrives.
#[test]
fn with_the_preference_off_an_agent_command_runs_at_once() {
    let (mut h, tx) = agent_harness();
    h.app.preferences.agent_off_thread = false;
    let features = h.app.session.doc.features.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    h.frames(1);
    assert!(h.app.agent_computing().is_none());
    assert!(hole.try_recv().is_ok_and(|r| r["ok"] == true), "answered in the same frame");
    assert_eq!(h.app.session.doc.features.len(), features + 1);
}

/// #1989: with the agent cursor shown, the pointer acts out the agent's design change while it
/// computes off the window thread. Meanwhile the window answers `ui.*` and inspection (it used to
/// answer nothing at all until the command had run: a client polling the window saw it frozen),
/// and the next design change waits its turn.
#[test]
fn the_window_answers_while_the_agent_cursor_acts_out_a_design_change() {
    let (mut h, tx) = agent_harness();
    h.app.ui.agent_cursor.show = true;
    h.frames(2);
    let features = h.app.session.doc.features.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    h.frames(1);
    assert!(h.app.agent.busy(), "the pointer acts it out");
    let view = ask(&tx, "ui.inspect", json!({}));
    let inspect = ask(&tx, "document.inspect", json!({}));
    let boss = ask(
        &tx,
        "engine.execute",
        json!({"command": "solid.box", "params": {"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"}}),
    );
    h.frames(1);
    assert!(h.app.agent.busy());
    assert!(view.try_recv().is_ok_and(|v| v["ok"] == true), "ui.* answered while the pointer acts out the command");
    assert!(boss.try_recv().is_err(), "the next design change waits its turn");
    assert_eq!(h.app.session.doc.features.len(), features, "nothing applied yet");
    assert!(reply_to(&mut h, &inspect)["ok"] == true, "inspection answered too, from a copy");
    let r = reply_to(&mut h, &hole);
    assert_eq!(r["ok"], true, "{r}");
    let r = reply_to(&mut h, &boss);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.session.doc.features.len(), features + 2, "both, in order");
}

/// #1989: the run on the window thread reuses what was computed ahead, even when the evaluation
/// cache dropped it meanwhile (a design with large bodies keeps only a few states there): it
/// computes nothing again, so the window waits only for the bookkeeping. A parameter that drives
/// the whole timeline, changed by the agent with the cursor off and with it shown.
#[test]
fn the_window_thread_run_reuses_what_was_computed_ahead() {
    for cursor in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        h.app.run("parameters.add", json!({"name": "len", "expression": "60 mm"})).unwrap();
        for (id, p) in [
            ("solid.box", json!({"length": "len", "width": 20, "height": 10})),
            ("solid.box", json!({"corner": ["len / 2", 5, 10], "length": 10, "width": 10, "height": 5, "operation": "join"})),
            ("solid.cylinder", json!({"base": ["len / 2 + 5", 10, -1], "axis": [0, 0, 1], "radius": 2, "height": 20, "operation": "cut"})),
        ] {
            h.app.run(id, p).unwrap();
        }
        h.app.apply_off_thread = Some(Duration::ZERO);
        h.app.preferences.agent_off_thread = true;
        h.app.ui.agent_cursor.show = cursor;
        let (tx, rx) = std::sync::mpsc::channel();
        h.app.control_rx = Some(rx);
        h.frames(2);
        let recomputed = h.app.session.model.recomputed;
        let change = ask(&tx, "engine.execute", json!({"command": "parameters.change", "params": {"name": "len", "expression": "70 mm"}}));
        let mut reply = None;
        for _ in 0..500 {
            // The cache drops everything the evaluation ahead puts there.
            for _ in 0..2 {
                *h.app.session.model.cache.lock().unwrap() = Default::default();
                std::thread::sleep(Duration::from_millis(5));
            }
            h.frames(1);
            if let Ok(v) = change.try_recv() {
                reply = Some(v);
                break;
            }
        }
        let r = reply.expect("answered");
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(h.app.session.model.recomputed, recomputed, "cursor {cursor}: nothing computed again on the window thread");
        let v = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
        let want = 70.0 * 200.0 + 500.0 - std::f64::consts::PI * 4.0 * 15.0;
        assert!((v - want).abs() < 1.0, "the design as changed: {v}");
    }
}

fn feature_ids(h: &Harness) -> Vec<u64> {
    h.app.session.doc.features.iter().map(|f| f.id).collect()
}

/// Volume of the block with the hole through it, once the boss is deleted.
fn block_with_hole() -> f64 {
    12000.0 - std::f64::consts::PI * 4.0 * 10.0
}

fn volume(h: &mut Harness) -> f64 {
    h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap()
}

/// The timeline's Delete checks what the delete breaks off the window thread (#2678): the design
/// is as it was while it checks, and the feature goes once the check is in.
#[test]
fn the_timelines_delete_checks_off_the_window_thread() {
    let mut h = block_boss_hole();
    let boss = feature_ids(&h)[1];
    crate::timeline::delete_features(&mut h.app, vec![boss]);
    assert!(h.app.computing_since().is_some(), "checking");
    assert!(h.app.session.doc.feature(boss).is_some(), "the design is as it was while it checks");
    until_settled(&mut h);
    assert!(h.app.session.doc.feature(boss).is_none(), "deleted once the check is in");
    assert!(h.app.dialog.is_none(), "nothing breaks, so nothing to confirm");
    assert!((volume(&mut h) - block_with_hole()).abs() < 1.0);
}

/// Delete on the selection (the Delete key) runs its dry run off the window thread too.
#[test]
fn delete_on_the_selection_checks_off_the_window_thread() {
    let mut h = block_boss_hole();
    let boss = feature_ids(&h)[1];
    h.app.session.selection = vec![solidcraft_engine::Sel::Feature { id: boss }];
    let params = crate::delete::selection_params(&h.app);
    crate::delete::request(&mut h.app, params);
    assert!(h.app.computing_since().is_some(), "checking");
    assert!(h.app.session.doc.feature(boss).is_some());
    until_settled(&mut h);
    assert!(h.app.session.doc.feature(boss).is_none());
    assert!(h.app.menu.confirm.is_none());
    assert!((volume(&mut h) - block_with_hole()).abs() < 1.0);
}

/// #9917: with the history rolled back, the check builds the features after the marker off the
/// window thread and keeps them; the confirmation carries them to the delete, which runs with
/// them instead of building that history again on the window thread.
#[test]
fn a_confirmed_delete_runs_with_what_its_check_kept() {
    let mut h = Harness::new();
    h.app.home.open = false;
    let sk = h.app.run("sketch.create", json!({"plane": "XY"})).unwrap()["sketch"].as_u64().unwrap();
    let side = h.app.run("sketch.line", json!({"points": [[0, 0], [40, 0], [30, 20], [10, 20]], "closed": true})).unwrap()["curves"][0]
        .as_str()
        .unwrap()
        .to_string();
    h.app.run("sketch.finish", json!({})).unwrap();
    h.app.run("solid.extrude", json!({"distance": 5, "name": "Plate"})).unwrap();
    for i in 0..3 {
        h.app.run("solid.cylinder", json!({"base": [12 + 6 * i, 8, -1], "radius": 1, "height": 10, "operation": "cut"})).unwrap();
    }
    h.app.run("timeline.roll_to", json!({"position": 1})).unwrap();
    h.app.run("sketch.edit", json!({"sketch": sk})).unwrap();
    h.app.apply_off_thread = Some(Duration::ZERO);
    crate::delete::request(&mut h.app, json!({"items": [{"type": "sketch_curve", "id": side}]}));
    assert!(h.app.computing_since().is_some(), "checking");
    until_settled(&mut h);
    let pending = h.app.menu.confirm.clone().expect("what the delete breaks after the marker is confirmed first");
    assert!(pending.report["would_fail"].as_array().is_some_and(|a| !a.is_empty()), "{}", pending.report);
    let kept = pending.kept.expect("the check's results go with the confirmation");
    let built = kept.len();
    assert!(built > 0, "the check kept the features after the marker it built");
    assert!(crate::delete::confirm(&mut h.app, true));
    let curves = |h: &Harness| h.app.session.model.state().sketch(sk).map(|s| s.sketch.curves.len()).unwrap_or(0);
    assert_eq!(curves(&h), 3, "deleted");
    assert_eq!(kept.len(), built, "the delete built nothing the check had not kept");
}

/// A design changed while its delete is checked is checked again before anything is deleted, and
/// a second Delete meanwhile is left out.
#[test]
fn a_design_changed_while_a_delete_is_checked_is_checked_again() {
    let mut h = block_boss_hole();
    let ids = feature_ids(&h);
    crate::timeline::delete_features(&mut h.app, vec![ids[1]]);
    crate::timeline::delete_features(&mut h.app, vec![ids[2]]);
    h.app.run("solid.box", json!({"corner": [100, 0, 0], "length": 10, "width": 10, "height": 10})).unwrap();
    until_settled(&mut h);
    assert!(h.app.session.doc.feature(ids[1]).is_none(), "the boss is deleted");
    assert!(h.app.session.doc.feature(ids[2]).is_some(), "the second Delete was left out");
    assert!((volume(&mut h) - (block_with_hole() + 1000.0)).abs() < 1.0, "the box added meanwhile stays");
}

/// Without a wait (headless use, tests) the check runs in place, as before.
#[test]
fn without_a_wait_the_delete_checks_in_place() {
    let mut h = block_boss_hole();
    h.app.apply_off_thread = None;
    let boss = feature_ids(&h)[1];
    crate::timeline::delete_features(&mut h.app, vec![boss]);
    assert!(h.app.delete_check.is_none());
    assert!(h.app.session.doc.feature(boss).is_none());
}

/// A body renamed in its rename box (#2467): off the window thread, the features after the one
/// that made it (a pattern, a chamfer) kept as they are, and the body's lock following the new
/// name.
#[test]
fn a_body_renamed_in_the_browser_keeps_the_pattern_after_it() {
    let mut h = Harness::new();
    h.app.home.open = false;
    for (id, p) in [
        ("sketch.create", json!({"plane": "XY"})),
        ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [80, 50]})),
        ("sketch.finish", json!({})),
        ("solid.extrude", json!({"distance": 10})),
        ("solid.hole", json!({"position": [15, 12, 10], "diameter": 5, "name": "Hole1"})),
        (
            "solid.pattern.rectangular",
            json!({"features": ["Hole1"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 25, "dir2": [0, 1, 0], "count2": 2, "spacing2": 25}),
        ),
        ("solid.chamfer", json!({"edges": [[40, 0, 10]], "distance": 1})),
    ] {
        h.app.run(id, p).unwrap();
    }
    h.app.session.visibility.locked_bodies.push("Body1".into());
    h.app.apply_off_thread = Some(Duration::ZERO);
    crate::context_menu::start_rename(&mut h.app, crate::context_menu::RenameWhat::Body("Body1".into()), egui::pos2(0.0, 0.0));
    h.frames(2);
    h.step(&json!({"text": "Plate"})).unwrap();
    let names = |h: &Harness| h.app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect::<Vec<_>>();
    let mesh = |h: &Harness| h.app.session.model.state().bodies.first().map(|b| b.mesh());
    let before = mesh(&h);
    // What Enter does, without the frames after it that would see the rename in.
    let rename = h.app.menu.rename.take().unwrap();
    crate::context_menu::commit_rename(&mut h.app, &rename);
    assert!(h.app.computing_since().is_some(), "computing");
    assert_eq!(names(&h), ["Body1"], "the design is as it was while it computes");
    until_settled(&mut h);
    assert_eq!(names(&h), ["Plate"]);
    let kept = before.zip(mesh(&h)).is_some_and(|(a, b)| std::sync::Arc::ptr_eq(&a, &b));
    assert!(kept, "no feature computed again");
    assert_eq!(h.app.session.visibility.locked_bodies, ["Plate"]);
}
