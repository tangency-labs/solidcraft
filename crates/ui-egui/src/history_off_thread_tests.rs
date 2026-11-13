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
    h.app.preferences.operations_panel = false;
    // A move that does not finish, with another waiting behind it.
    let (job, _keep) = crate::deferred::Deferred::never_done("timeline.roll_to", json!({"position": 1}), h.app.session.revision);
    h.app.deferred = Some(job);
    h.app.deferred_queue.push(("timeline.roll_to".into(), json!({"position": 2}), 0));
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
fn finishing_a_sketch_in_the_background_restores_the_design_and_its_undo_snapshot() {
    let (mut h, sketches) = sketches_with_later_bodies();
    let before = h.app.session.doc.to_json();
    h.app.edit_sketch(sketches[0]);
    h.app.run("sketch.line", json!({"points": [[0, 2], [10, 2]]})).unwrap();
    h.app.finish_sketch();
    assert!(h.app.deferred.is_some());
    until_settled(&mut h);
    assert_eq!(marker(&h), None);
    assert_eq!(h.app.session.active_sketch, None);
    h.app.run("edit.undo", json!({})).unwrap();
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!(h.app.session.active_sketch, None);
}

#[test]
fn a_pending_sketch_exit_cannot_close_the_next_editor() {
    for cancel in [false, true] {
        let (mut h, sketches) = sketches_with_later_bodies();
        let curves = h.app.session.doc.sketch(sketches[0]).unwrap().curves.len();
        h.app.edit_sketch(sketches[0]);
        h.app.run("sketch.line", json!({"points": [[0, 2], [10, 2]]})).unwrap();
        if cancel {
            h.app.cancel_sketch();
        } else {
            h.app.finish_sketch();
        }
        assert!(h.app.deferred.is_some());
        h.app.edit_sketch(sketches[1]);
        until_settled(&mut h);
        assert_eq!(h.app.session.active_sketch, Some(sketches[1]));
        assert_eq!(marker(&h), Some(3));
        assert_eq!(h.app.session.doc.sketch(sketches[0]).unwrap().curves.len(), curves + usize::from(!cancel));
        h.app.finish_sketch();
        until_settled(&mut h);
        assert_eq!(h.app.session.active_sketch, None);
        assert_eq!(marker(&h), None);
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
fn starting_forms_waits_for_the_sketch_to_restore_its_history() {
    let (mut h, sketches) = sketches_with_later_bodies();
    h.app.edit_sketch(sketches[0]);
    until_settled(&mut h);
    assert_eq!(h.app.session.active_sketch, Some(sketches[0]));
    assert!(marker(&h).is_some());
    h.app.start("form.prototype.create");
    assert!(h.app.session.forms_prototype.is_none(), "the Form waits for the sketch to finish");
    assert_eq!(h.app.start_after_roll.as_ref().map(|(id, _)| id.as_str()), Some("form.prototype.create"));
    until_settled(&mut h);
    assert!(h.app.session.active_sketch.is_none());
    assert!(h.app.session.forms_prototype.is_some());
    assert_eq!(marker(&h), None);
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
/// step's command (`2/3 · Cylinder`); a single command shows just its name. The chip is what
/// shows with the operations panel off.
#[test]
fn a_computing_batch_says_its_step() {
    let (mut h, tx) = agent_harness();
    h.app.preferences.operations_panel = false;
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

/// The tab of a design whose command is still computing closes: the requests behind it are for
/// the design shown now, and don't wait for a computation that may take minutes. The worker is
/// held open here by an agent command that never finishes on this frame's clock: it is refused
/// on the next frame, whether or not its result has come in.
#[test]
fn an_agent_command_for_a_closed_design_is_refused_at_once_and_holds_up_nothing() {
    let (mut h, tx) = agent_harness();
    let hole = ask(&tx, "engine.execute", second_hole());
    h.frames(1);
    assert!(h.app.agent_computing().is_some());
    crate::documents::close(&mut h.app, 0, true);
    h.frames(1);
    let r = hole.try_recv().expect("refused on the next frame");
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().is_some_and(|e| e.contains("replaced")), "{r}");
    assert!(h.app.agent_computing().is_none());
    let next = ask(&tx, "engine.execute", json!({"command": "solid.box", "params": {"length": 10, "width": 10, "height": 10}}));
    assert_eq!(reply_to(&mut h, &next)["ok"], true);
    assert_eq!(h.app.session.doc.features.len(), 1, "only the new design's box");
}

/// A history command of a design closed while it computes (the last tab: a blank design takes its
/// place), and those waiting behind it, go with it: the new design starts with nothing computing,
/// its Extrude from a finished sketch doesn't wait for them, and their late result is dropped.
#[test]
fn a_design_closed_while_its_history_computes_holds_up_nothing() {
    let mut h = block_boss_hole();
    let (job, late) = crate::deferred::Deferred::never_done("timeline.roll_to", json!({"position": 1}), h.app.session.revision);
    h.app.deferred = Some(job);
    h.app.deferred_queue.push(("timeline.roll_to".into(), json!({"position": 2}), 0));
    h.app.start_after_roll = Some(("solid.extrude".into(), 1));
    crate::documents::close(&mut h.app, 0, true);
    assert!(h.app.computing_since().is_none(), "the blank design has nothing computing");
    assert!(h.app.deferred.is_none() && h.app.deferred_queue.is_empty() && h.app.start_after_roll.is_none());
    h.app.home.open = false;
    h.app.run("sketch.create", json!({"plane": "XY"})).unwrap();
    h.app.run("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]})).unwrap();
    h.app.start("solid.extrude");
    assert!(h.app.start_after_roll.is_none(), "the extrude waits for nothing");
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.command()), Some("solid.extrude"));
    assert!(late.send(None).is_err(), "nobody is left to take the closed design's result");
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().and_then(|d| d.command()), Some("solid.extrude"));
}

/// Closing a design or replacing it stops the kernel work still computing for it; showing another
/// tab doesn't, since that design is still there to come back to.
#[test]
fn closing_a_design_stops_what_it_has_computing_and_a_tab_switch_does_not() {
    let mut h = block_boss_hole();
    let first = h.app.session.stop_token();
    let copy = h.app.session.ahead();
    crate::documents::new_tab(&mut h.app);
    assert!(!first.is_stopped(), "only another tab shown");
    let second = h.app.session.stop_token();
    assert!(!second.is_stopped());
    crate::documents::close(&mut h.app, 0, true);
    assert!(first.is_stopped(), "the first design closed");
    assert!(!second.is_stopped(), "this design goes on");
    assert!(
        copy.replies(&[("solid.box".to_string(), json!({"length": 1, "width": 1, "height": 1}))]).iter().all(Result::is_err),
        "its job gets nowhere"
    );
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.app.run("file.new", json!({})).unwrap();
    assert!(second.is_stopped(), "replaced in its tab");
    assert!(!h.app.session.stop_token().is_stopped());
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    crate::documents::close(&mut h.app, 0, true);
    assert!(!h.app.session.stop_token().is_stopped(), "the blank design that took the last tab's place");
}

/// A design's history command stays with its tab: another tab doesn't wait for it and doesn't get
/// its result, even once it is in. Shown again, the design picks it up.
#[test]
fn a_history_command_stays_with_its_tab() {
    let mut h = block_boss_hole();
    let (job, done) = crate::deferred::Deferred::never_done("timeline.roll_to", json!({"position": 1}), h.app.session.revision);
    h.app.deferred = Some(job);
    h.app.deferred_queue.push(("timeline.roll_to".into(), json!({"position": 2}), 0));

    crate::documents::new_tab(&mut h.app);
    assert_eq!(h.app.docs.active, 1);
    assert!(h.app.computing_since().is_none(), "the new tab has nothing computing");
    assert!(h.app.deferred.is_none() && h.app.deferred_queue.is_empty());
    // The other design's command finishes while this one is shown: nothing lands here.
    done.send(None).unwrap();
    h.app.run("solid.box", json!({"length": 10, "width": 10, "height": 10})).unwrap();
    h.app.run_heavy("timeline.roll_to", json!({"position": 0})).unwrap();
    until_settled(&mut h);
    assert_eq!(h.app.session.doc.features.len(), 1, "this tab's box and nothing else");
    assert_eq!(marker(&h), Some(0), "this tab's own marker move ran at once");

    crate::documents::switch(&mut h.app, 0);
    assert_eq!(h.app.deferred_queue.len(), 1, "the first tab's command and the one behind it came back with it");
    assert!(h.app.computing_since().is_some());
    until_settled(&mut h);
    assert_eq!(marker(&h), Some(2), "they ran on the design they were asked for");
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

/// A control request for the app, where its reply lands, and its ticket (what its connection
/// cancels with `control.cancel`).
fn ask_ticket(
    tx: &std::sync::mpsc::Sender<crate::ControlRequest>,
    method: &str,
    params: serde_json::Value,
) -> (std::sync::mpsc::Receiver<serde_json::Value>, std::sync::Arc<crate::control_job::Ticket>) {
    let (req, reply) = crate::ControlRequest::new(method, params);
    let ticket = req.ticket.clone();
    tx.send(req).unwrap();
    (reply, ticket)
}

fn boss_step() -> serde_json::Value {
    json!({"command": "solid.box", "params": {"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"}})
}

/// #1989: the agent cancels its design change while it computes off the window thread
/// (`control.cancel`): stopped as Stop stops it, nothing applied, its worker told to stop, and
/// the requests behind it go on.
#[test]
fn cancelling_an_agent_command_while_it_computes_applies_nothing() {
    use crate::control_job::Cancel;
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let undo = h.app.session.undo.len();
    let (hole, ticket) = ask_ticket(&tx, "engine.execute", second_hole());
    let boss = ask(&tx, "engine.execute", boss_step());
    h.frames(1);
    assert!(h.app.agent_computing().is_some());
    assert_eq!(ticket.cancel(), Cancel::Stopped { writes_file: false });
    assert_eq!(ticket.cancel(), Cancel::Already, "once");
    let r = reply_to(&mut h, &hole);
    assert!(r["ok"] == false && r["stopped"] == true, "{r}");
    assert!(r["error"].as_str().is_some_and(|e| e.contains("control.cancel") && e.contains("nothing was applied")), "{r}");
    let r = reply_to(&mut h, &boss);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(h.app.session.doc.features.len(), features + 1, "only the boss");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert!(h.app.agent_computing().is_none());
}

/// A request still waiting its turn behind one that computes is removed when its client cancels
/// it: answered as cancelled, never run. One that already ran can't be cancelled.
#[test]
fn a_queued_agent_request_cancelled_never_runs() {
    use crate::control_job::Cancel;
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    let (boss, ticket) = ask_ticket(&tx, "engine.execute", boss_step());
    h.frames(1);
    assert!(h.app.agent_computing().is_some() && boss.try_recv().is_err(), "the boss waits its turn");
    assert_eq!(ticket.cancel(), Cancel::Removed);
    let r = reply_to(&mut h, &hole);
    assert_eq!(r["ok"], true, "{r}");
    let r = reply_to(&mut h, &boss);
    assert!(r["ok"] == false && r["cancelled"] == true, "{r}");
    assert!(r["error"].as_str().is_some_and(|e| e.starts_with("cancelled:") && e.contains("nothing was applied")), "{r}");
    until_settled(&mut h);
    assert_eq!(h.app.session.doc.features.len(), features + 1, "only the hole");
    // With the preference off it runs on the window thread at once: done, it can't be cancelled.
    h.app.preferences.agent_off_thread = false;
    let (boss, ticket) = ask_ticket(&tx, "engine.execute", boss_step());
    h.frames(1);
    assert!(boss.try_recv().is_ok_and(|r| r["ok"] == true));
    assert_eq!(ticket.cancel(), Cancel::Running);
    assert_eq!(h.app.session.doc.features.len(), features + 2);
}

/// With the agent cursor shown, a design change the pointer acts out can be cancelled until it
/// runs: nothing applied, and the pointer is free for the next request.
#[test]
fn cancelling_a_design_change_the_agent_cursor_acts_out_applies_nothing() {
    use crate::control_job::Cancel;
    let (mut h, tx) = agent_harness();
    h.app.ui.agent_cursor.show = true;
    h.frames(2);
    let features = h.app.session.doc.features.len();
    let (hole, ticket) = ask_ticket(&tx, "engine.execute", second_hole());
    h.frames(1);
    assert!(h.app.agent.busy(), "the pointer acts it out");
    assert_eq!(ticket.cancel(), Cancel::Stopped { writes_file: false });
    let r = reply_to(&mut h, &hole);
    assert!(r["ok"] == false && r["stopped"] == true, "{r}");
    assert!(!h.app.agent.busy());
    let boss = ask(&tx, "engine.execute", boss_step());
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
    let _ = crate::context_menu::commit_rename(&mut h.app, &rename);
    assert!(h.app.computing_since().is_some(), "computing");
    assert_eq!(names(&h), ["Body1"], "the design is as it was while it computes");
    until_settled(&mut h);
    assert_eq!(names(&h), ["Plate"]);
    let kept = before.zip(mesh(&h)).is_some_and(|(a, b)| std::sync::Arc::ptr_eq(&a, &b));
    assert!(kept, "no feature computed again");
    assert_eq!(h.app.session.visibility.locked_bodies, ["Plate"]);
}

/// With the operations panel (the default), an agent's computing batch is a row named after the
/// step's command, with how many of the batch's steps are done, and no chip; its X stops it as
/// the chip's Stop did: answered as stopped, nothing applied.
#[test]
fn a_computing_batch_is_a_row_of_the_operations_panel_that_stops_it() {
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let hole = |x: f64| json!({"command": "solid.cylinder", "params": {"base": [x, 10, -1], "axis": [0, 0, 1], "radius": 3, "height": 20, "operation": "cut"}});
    let batch = ask(&tx, "engine.batch", json!({"commands": [hole(50.0), hole(30.0), hole(70.0)]}));
    h.frames(1);
    assert!(h.app.agent_computing().is_some(), "the batch computes");
    let rows = crate::operations::current(&h.app);
    let row = rows.iter().find(|r| r.key == "agent").expect("a row for the agent's command");
    assert!(!row.label.is_empty() && !row.label.contains('.'), "the toolbar's name: {row:?}");
    assert_eq!(row.steps.map(|(_, total)| total), Some(3), "{row:?}");
    assert!(row.cancellable());
    assert!(crate::scenario::rect_of("agent:chip").is_none(), "no chip besides the row");
    let steps = h.app.agent_steps().expect("its steps");
    crate::operations::cancel(&mut h.app, "agent").unwrap();
    assert!(steps.progress.is_cancelled(), "its computation stops, not left to compute unseen");
    let reply = reply_to(&mut h, &batch);
    assert_eq!((reply["ok"].clone(), reply["stopped"].clone()), (json!(false), json!(true)), "{reply}");
    h.frames(2);
    assert_eq!(h.app.session.doc.features.len(), features, "nothing applied");
    let finished = crate::operations::control(&mut h.app, &json!({})).unwrap()["finished"].clone();
    assert_eq!(finished[0]["key"], "agent");
    assert_eq!(finished[0]["outcome"], "cancelled", "{finished}");
    // One that computes to its end is listed as done, its reply passed on as ever.
    let single = ask(&tx, "engine.execute", second_hole());
    h.frames(1);
    assert_eq!(reply_to(&mut h, &single)["ok"], true);
    h.frames(1);
    let finished = crate::operations::control(&mut h.app, &json!({})).unwrap()["finished"].clone();
    if h.app.operations.rows(crate::now_ms()).any(|(r, _)| r.key == "agent") || finished[0]["key"] == "agent" {
        assert_eq!(finished[0]["outcome"], "done", "{finished}");
    }
}

/// A batch stopped by a failing step answers `ok` with the step in `failed`: the operation is
/// listed as failed, saying why, not as done.
#[test]
fn a_batch_stopped_by_a_failing_step_ends_failed() {
    let (mut h, tx) = agent_harness();
    let hole = |x: f64| json!({"command": "solid.cylinder", "params": {"base": [x, 10, -1], "axis": [0, 0, 1], "radius": 3, "height": 20, "operation": "cut"}});
    let bad = json!({"command": "solid.fillet", "params": {"edges": ["no-such-edge"], "radius": 2}});
    let batch = ask(&tx, "engine.batch", json!({"commands": [hole(50.0), hole(30.0), bad]}));
    h.frames(1);
    assert!(h.app.agent_computing().is_some(), "the batch computes off the window thread");
    let reply = reply_to(&mut h, &batch);
    assert!(reply["result"]["failed"].is_object(), "{reply}");
    h.frames(1);
    let finished = crate::operations::control(&mut h.app, &json!({})).unwrap()["finished"].clone();
    assert_eq!(finished[0]["key"], "agent", "{finished}");
    assert_eq!(finished[0]["outcome"], "failed", "{finished}");
    assert!(finished[0]["error"].as_str().is_some_and(|e| e.contains("Fillet") || e.contains("edge")), "{finished}");
}

/// With the agent cursor shown, a batch it acts out is computed off the window thread, also when
/// a person's edit goes first meanwhile: the run for real finds every feature computed and
/// computes none on the window thread.
#[test]
fn a_batch_the_cursor_acts_out_computes_nothing_on_the_window_thread() {
    for edit in [false, true] {
        let (mut h, tx) = agent_harness();
        h.app.ui.agent_cursor = crate::agent_cursor::Settings { show: true, speed: crate::agent_cursor::Speed::Times(5.0), follow_camera: false };
        let features = h.app.session.doc.features.len();
        let hole = |x: f64| json!({"command": "solid.cylinder", "params": {"base": [x, 10, -1], "axis": [0, 0, 1], "radius": 2, "height": 20, "operation": "cut"}});
        let batch = ask(&tx, "engine.batch", json!({"commands": [hole(50.0), hole(30.0)]}));
        h.frames(1);
        assert!(h.app.agent.busy(), "the pointer acts the batch out");
        if edit {
            h.app.run("solid.box", json!({"corner": [40, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"})).unwrap();
        }
        let recomputed = h.app.session.model.recomputed;
        let r = reply_to(&mut h, &batch);
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(h.app.session.doc.features.len(), features + 2 + usize::from(edit), "edit {edit}");
        assert_eq!(h.app.session.model.recomputed, recomputed, "edit {edit}: nothing computed on the window thread");
    }
}

#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn plugin_package_for_worker_test(tag: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("solidcraft-plugin-worker-{}-{tag}", std::process::id()));
    let dir = root.join("sample");
    std::fs::create_dir_all(&dir).unwrap();
    let (package, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
    let wat = include_str!("plugin_fixture.wat").replace("PLUGIN_PACKAGE", package).replace("PLUGIN_VERSION", version);
    std::fs::write(dir.join("plugin.wasm"), wat::parse_str(&wat).unwrap()).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        json!({"id":"sample","name":"Worker sample","version":"1","capabilities":["commands@0.1"],"runtime":{
        "module":"plugin.wasm","entries":["make"],"commands":[]}})
        .to_string(),
    )
    .unwrap();
    root
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn plugin_execution_is_off_thread_even_with_agent_off_thread_disabled() {
    let (mut h, tx) = agent_harness();
    h.app.preferences.agent_off_thread = false;
    h.app.run("solid.box", json!({"length":1,"width":1,"height":1,"corner":[100,100,100]})).unwrap();
    h.app.run("edit.undo", json!({})).unwrap();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    let own_level = h.app.session.param_check;
    let root = plugin_package_for_worker_test("responsive");
    let path = root.join("sample").to_string_lossy().into_owned();
    let inspection = ask(&tx, "engine.execute", json!({"command":"plugin.inspect","params":{"path":path}}));
    let inspection = reply_to(&mut h, &inspection);
    assert_eq!(inspection["ok"], true, "{inspection}");
    let inspected = &inspection["result"];
    assert_eq!(h.app.session.plugin_state.packages.get("sample"), Some(&path), "async inspection registers namespaced entries");
    assert_eq!(
        h.app.run("PLUGIN.RUN", json!({"path":path,"approval":inspected["approval"],"entry":"make"})).unwrap_err(),
        h.app.tr("ui.plugin_standalone")
    );
    let (release, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    let request =
        ask(&tx, "engine.execute", json!({"command":"sample.make","param_check":"strict","params":{"approval":inspected["approval"],"params":{}}}));
    h.frames(1);
    assert!(h.app.agent_computing().is_some());
    let view = ask(&tx, "ui.inspect", json!({}));
    h.frames(1);
    assert_eq!(view.try_recv().unwrap()["ok"], true, "window answers while external code waits");
    release.send(()).unwrap();
    let reply = reply_to(&mut h, &request);
    assert_eq!(reply["ok"], true, "{reply}");
    assert_eq!(reply["param_check"], "strict");
    assert_eq!(h.app.session.param_check, own_level);
    assert_eq!(reply["result"]["result"], serde_json::Value::Null);
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history, "a read-only plugin preserves history");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn a_plugin_result_is_discarded_after_design_or_selection_changes() {
    for context in ["document", "selection", "plugin_limits"] {
        let (mut h, tx) = agent_harness();
        h.app.preferences.language = if context == "selection" { "ja" } else { "en" }.into();
        h.app.run("solid.box", json!({"length":1,"width":1,"height":1,"corner":[200,200,200]})).unwrap();
        h.app.run("edit.undo", json!({})).unwrap();
        assert!(!h.app.session.redo.is_empty());
        let root = plugin_package_for_worker_test(&format!("stale-{context}"));
        let path = root.join("sample").to_string_lossy().into_owned();
        let inspected = h.app.run("plugin.inspect", json!({"path":path})).unwrap();
        let (release, gate) = std::sync::mpsc::channel();
        h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
        let request =
            ask(&tx, "engine.execute", json!({"command":"plugin.run","params":{"path":path,"approval":inspected["approval"],"entry":"make"}}));
        h.frames(1);
        assert!(h.app.agent_computing().is_some());
        if context == "document" {
            h.app.run("solid.box", json!({"length":2,"width":2,"height":2,"corner":[100,100,100]})).unwrap();
        } else if context == "plugin_limits" {
            crate::prefs::set(&mut h.app, &json!({"plugin_max_memory_mib":512})).unwrap();
        } else {
            let selection = vec![solidcraft_engine::Sel::Feature { id: feature_ids(&h)[0] }];
            if h.app.session.selection == selection {
                h.app.session.selection.clear();
            } else {
                h.app.session.selection = selection;
            }
        }
        let before = h.app.session.doc.to_json();
        let history = (h.app.session.undo.len(), h.app.session.redo.len());
        release.send(()).unwrap();
        let reply = reply_to(&mut h, &request);
        assert_eq!(reply["ok"], false, "{context}: {reply}");
        let language = solidcraft_ui_text::Locale::for_language(&h.app.preferences.language);
        assert_eq!(reply["error"], language.text("ui.plugin_context_changed"), "{context}: {reply}");
        assert_eq!(h.app.session.doc.to_json(), before);
        assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn cancelling_a_plugin_worker_answers_stopped_without_adopting_its_result() {
    let (mut h, tx) = agent_harness();
    let root = plugin_package_for_worker_test("cancelled");
    let path = root.join("sample").to_string_lossy().into_owned();
    let inspected = h.app.run("plugin.inspect", json!({"path":path})).unwrap();
    let (release, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    let before = h.app.session.doc.to_json();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    let (reply, ticket) =
        ask_ticket(&tx, "engine.execute", json!({"command":"plugin.run","params":{"path":path,"approval":inspected["approval"],"entry":"make"}}));
    h.frames(1);
    assert_eq!(ticket.cancel(), crate::control_job::Cancel::Stopped { writes_file: true });
    let response = reply_to(&mut h, &reply);
    assert_eq!((response["ok"].clone(), response["stopped"].clone()), (json!(false), json!(true)), "{response}");
    assert!(response["error"].as_str().unwrap().contains("file may still be written"), "{response}");
    release.send(()).unwrap();
    h.frames(2);
    assert_eq!(h.app.session.doc.to_json(), before);
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn plugin_commands_in_batches_and_scripts_are_refused_before_an_earlier_edit() {
    let mut h = Harness::new();
    let before = h.app.session.doc.to_json();
    for command in ["plugin.run", "PLUGIN.RUN"] {
        for method in ["engine.batch", "engine.script"] {
            let reply = h.call(
                method,
                json!({"commands":[
                    {"command":"solid.box","params":{"length":2,"width":3,"height":4}},
                    {"command":command,"params":{}}
                ]}),
            );
            assert_eq!(reply["ok"], false, "{method}: {reply}");
            assert_eq!(reply["error"], h.app.tr("ui.plugin_standalone"), "{method}: {reply}");
            assert_eq!(h.app.session.doc.to_json(), before, "{method}: {command}");
            assert!(h.app.session.undo.is_empty());
        }
    }
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn plugin_preflight_checks_namespaced_calls_after_ten_thousand_steps() {
    let mut h = Harness::new();
    let root = plugin_package_for_worker_test("long-script");
    let inspected = h.app.run("plugin.inspect", json!({"path":root.join("sample")})).unwrap();
    let before = h.app.session.doc.to_json();
    let mut steps = vec![json!({"command":"document.inspect","params":{}}); 10_000];
    steps[0] = json!({"command":"solid.box","params":{"length":2,"width":3,"height":4}});
    steps.push(json!({"command":"sample.make","params":{"approval":inspected["approval"]}}));
    let (reply, _) = h.call_queued("engine.script", json!({"commands":steps}));
    assert_eq!(reply["error"], h.app.tr("ui.plugin_standalone"), "{reply}");
    assert_eq!(h.app.session.doc.to_json(), before);
    assert!(h.app.session.undo.is_empty());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn plugin_inspections_reject_another_session_and_older_results() {
    for context in ["tab", "worker", "direct"] {
        let (mut h, tx) = agent_harness();
        let root = plugin_package_for_worker_test(&format!("inspect-{context}"));
        let path = root.join("sample").to_string_lossy().into_owned();
        let (release, gate) = std::sync::mpsc::channel();
        h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
        let first = ask(&tx, "engine.execute", json!({"command":"plugin.inspect","params":{"path":path}}));
        h.frames(1);
        assert_eq!(h.app.control_answering.len(), 1);
        h.app.apply_gate = None;
        let newer_root = if context == "tab" {
            crate::documents::open_tab(&mut h.app, solidcraft_engine::Session::default()).unwrap();
            None
        } else {
            let newer_root = plugin_package_for_worker_test(&format!("inspect-new-{context}"));
            if context == "direct" {
                h.app.run("PLUGIN.INSPECT", json!({"path":newer_root.join("sample")})).unwrap();
            } else {
                let newer = ask(&tx, "engine.execute", json!({"command":"plugin.inspect","params":{"path":newer_root.join("sample")}}));
                assert_eq!(reply_to(&mut h, &newer)["ok"], true);
            }
            Some(newer_root)
        };
        release.send(()).unwrap();
        let reply = reply_to(&mut h, &first);
        assert_eq!(reply["ok"], false, "{reply}");
        assert_eq!(reply["error"], h.app.tr("ui.plugin_context_changed"));
        if let Some(newer_root) = newer_root {
            assert_eq!(h.app.session.plugin_state.packages["sample"], newer_root.join("sample").to_string_lossy());
            let _ = std::fs::remove_dir_all(newer_root);
        } else {
            assert!(!h.app.session.plugin_state.packages.contains_key("sample"));
        }
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn stopping_a_plugin_at_the_window_warns_about_possible_file_writes() {
    let (mut h, tx) = agent_harness();
    let root = plugin_package_for_worker_test("stop-warning");
    let path = root.join("sample");
    let inspected = h.app.run("plugin.inspect", json!({"path":path})).unwrap();
    let (release, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    let reply = ask(&tx, "engine.execute", json!({"command":"plugin.run","params":{"path":path,"entry":"make","approval":inspected["approval"]}}));
    h.frames(1);
    h.app.stop_agent_command();
    let response = reply.try_recv().unwrap();
    assert_eq!(response["stopped"], true);
    assert!(response["error"].as_str().unwrap().contains("file may still be written"), "{response}");
    release.send(()).unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cancelled_plugin_display_warmup_leaves_parallel_body_meshes_cold() {
    let mut session = solidcraft_engine::Session::default();
    for i in 0..17 {
        session.execute("solid.box", &json!({"length":1,"width":1,"height":1,"corner":[i*3,0,0]})).unwrap();
    }
    let prepared = session.scratch().compute_commands(&[], None).unwrap();
    let progress = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    let worker_progress = progress.clone();
    let (release, gate) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        gate.recv().unwrap();
        crate::control_job::warm_plugin_display(&prepared, solidcraft_engine::doc::display_quality::DisplayQuality::Normal, &worker_progress)
            .unwrap();
        prepared
    });
    progress.cancel();
    release.send(()).unwrap();
    session.accept_commands(worker.join().unwrap()).unwrap();
    assert_eq!(session.model.state().bodies.len(), 17);
    assert!(session.model.state().bodies.iter().all(|body| body.cached_mesh().is_none()), "a stop must reach the parallel display workers");
}

#[test]
#[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
fn accepted_plugin_sketch_exits_clear_the_window_tool() {
    for command in ["sketch.finish", "sketch.cancel"] {
        let (mut h, tx) = agent_harness();
        let sketch = h.app.run("sketch.create", json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
        h.app.tool = crate::tools::Tool::for_command("sketch.line");
        assert!(h.app.tool.is_some());
        let root = plugin_package_for_worker_test(command);
        let path = root.join("sample");
        let (package, version) = solidcraft_engine::plugin::API.rsplit_once('@').unwrap();
        let open_args = json!({"sketch":sketch,"restore_marker":false}).to_string();
        let open = format!(
            "(func $open i32.const 2048 i32.const 11 i32.const 3072 i32.const {} i32.const 0 call $run)\n    (func $execute",
            open_args.len()
        );
        let wat = include_str!("../../engine/src/plugin/fixture.wat")
            .replace("__PACKAGE__", package)
            .replace("__VERSION__", version)
            .replace("call $box\n      call $box", "call $open\n      call $box")
            .replacen("(func $execute (;3;)", &format!("{open} (;3;)"), 1)
            .replace("file.export", "sketch.edit")
            .replacen("    (@producers", &format!("    (data (i32.const 3072) \"{}\")\n    (@producers", open_args.replace('"', r"\22")), 1)
            .replace(
                "i32.const 9\n      i32.const 1024\n      i32.const 33",
                &format!("i32.const {}\n      i32.const 1024\n      i32.const 2", command.len()),
            )
            .replace("solid.box", command)
            .replace(r#"{\22length\22:2,\22width\22:3,\22height\22:4}"#, "{}");
        std::fs::write(path.join("plugin.wasm"), wat::parse_str(wat).unwrap()).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path.join("plugin.json")).unwrap()).unwrap();
        manifest["runtime"]["commands"] = json!(["sketch.edit", command]);
        std::fs::write(path.join("plugin.json"), manifest.to_string()).unwrap();
        let inspected = h.app.run("plugin.inspect", json!({"path":path})).unwrap();
        let request =
            ask(&tx, "engine.execute", json!({"command":"plugin.run","params":{"path":path,"entry":"make","approval":inspected["approval"]}}));
        let reply = reply_to(&mut h, &request);
        assert_eq!(reply["ok"], true, "{command}: {reply}");
        assert!(h.app.session.active_sketch.is_none());
        assert!(h.app.tool.is_none(), "{command} must perform sketch window cleanup");
        let _ = std::fs::remove_dir_all(root);
    }
}

/// The agent's row of the operations panel and the rows queued behind it, as listed by
/// `ui.operations` (newest first).
fn agent_rows(h: &mut Harness) -> Vec<serde_json::Value> {
    let listed = crate::operations::control(&mut h.app, &json!({})).unwrap();
    let rows = listed["operations"].as_array().unwrap().clone();
    rows.into_iter().filter(|r| r["key"].as_str().is_some_and(|k| k.starts_with("agent"))).collect()
}

/// A design change waiting behind the agent's computing one is a row of its own until it runs,
/// and its X cancels just that one: answered as cancelled, never run, the others still run. A
/// request that does not change the design is no row.
#[test]
fn an_agent_request_waiting_its_turn_is_a_row_the_x_cancels() {
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    let boss = ask(&tx, "engine.execute", boss_step());
    let last = ask(
        &tx,
        "engine.execute",
        json!({"command": "solid.box", "params": {"corner": [70, 5, 10], "length": 5, "width": 5, "height": 5, "operation": "join"}}),
    );
    h.frames(1);
    assert!(h.app.agent_computing().is_some() && boss.try_recv().is_err());
    let rows = agent_rows(&mut h);
    let held: Vec<_> = rows.iter().filter(|r| r["queued"] == true).collect();
    assert!(held.len() == 2 && held.iter().all(|r| r["cancellable"] == true && !r["label"].as_str().unwrap().is_empty()), "{rows:?}");
    // The first one waiting (newest first: the last in the list) is the boss.
    let key = held[1]["key"].as_str().unwrap().to_string();
    assert!(key.starts_with("agent:held:"));
    crate::operations::cancel(&mut h.app, &key).unwrap();
    let r = boss.try_recv().expect("answered at once");
    assert!(r["ok"] == false && r["cancelled"] == true, "{r}");
    assert_eq!(agent_rows(&mut h).iter().filter(|r| r["queued"] == true).count(), 1);
    assert_eq!(reply_to(&mut h, &hole)["ok"], true);
    assert_eq!(reply_to(&mut h, &last)["ok"], true);
    until_settled(&mut h);
    assert_eq!(h.app.session.doc.features.len(), features + 2, "the hole and the last box; the boss never ran");
    assert!(agent_rows(&mut h).is_empty(), "all ran");
    // A request that only reads is no row of the agent's work.
    let (req, _reply) = crate::ControlRequest::new("ui.status", json!({}));
    h.app.control_held.push_back(req);
    assert!(agent_rows(&mut h).is_empty());
}

/// An agent that queues dozens of design changes still gets the operations panel: it shows the
/// first few waiting ones and one row standing for the rest, whose X cancels exactly those. Every
/// waiting one stays listed, and cancellable by its own key, over the control channel.
#[test]
fn many_waiting_agent_commands_keep_the_panel_and_fold_into_one_row() {
    use crate::operations::MAX_WAITING_SHOWN;
    let (mut h, tx) = agent_harness();
    let features = h.app.session.doc.features.len();
    let hole = ask(&tx, "engine.execute", second_hole());
    let waiting: Vec<_> = (0..40).map(|_| ask(&tx, "engine.execute", boss_step())).collect();
    h.frames(1);
    assert!(h.app.agent_computing().is_some());
    assert!(h.app.operations.panel_drew, "the panel is drawn with 40 commands waiting");
    let shown = h.app.operations.rows(crate::now_ms()).count();
    assert_eq!(shown, 1 + MAX_WAITING_SHOWN + 1, "the computing one, the first waiting ones and the summary row");
    let rows = agent_rows(&mut h);
    assert_eq!(rows.iter().filter(|r| r["queued"] == true).count(), 40, "all of them stay listed");
    // The summary row's X cancels those past the shown ones, and only them.
    crate::operations::cancel(&mut h.app, "agent:held:more").unwrap();
    let status = h.app.status.as_ref().map(|s| s.0.clone());
    assert_eq!(status.as_deref(), Some("36 waiting commands cancelled"), "one message for the whole action");
    for (i, w) in waiting.iter().enumerate() {
        match w.try_recv() {
            Ok(r) => assert!(i >= MAX_WAITING_SHOWN && r["cancelled"] == true, "{i}: {r}"),
            Err(_) => assert!(i < MAX_WAITING_SHOWN, "{i} was not answered"),
        }
    }
    assert_eq!(reply_to(&mut h, &hole)["ok"], true);
    for w in &waiting[..MAX_WAITING_SHOWN] {
        assert_eq!(reply_to(&mut h, w)["ok"], true);
    }
    until_settled(&mut h);
    assert_eq!(h.app.session.doc.features.len(), features + 1 + MAX_WAITING_SHOWN, "none of the cancelled ones ran");
}

/// Waiting rows stay in queue order with the summary row above them all, also when the queue
/// advances and a folded command unfolds into a row of its own.
#[test]
fn the_summary_row_stays_above_the_waiting_rows_when_one_unfolds() {
    use crate::operations::MAX_WAITING_SHOWN;
    let (mut h, tx) = agent_harness();
    let _hole = ask(&tx, "engine.execute", second_hole());
    let _waiting: Vec<_> = (0..MAX_WAITING_SHOWN + 2).map(|_| ask(&tx, "engine.execute", boss_step())).collect();
    h.frames(1);
    // The first waiting one (newest first: the last in the list) goes, and the next unfolds.
    let rows = agent_rows(&mut h);
    let first = rows.iter().rfind(|r| r["queued"] == true).unwrap()["key"].as_str().unwrap().to_string();
    crate::operations::cancel(&mut h.app, &first).unwrap();
    h.frames(1);
    let shown: Vec<_> = h.app.operations.rows(crate::now_ms()).map(|(r, alpha)| (r.key.clone(), r.label.clone(), alpha)).collect();
    assert_eq!(shown.last().map(|r| r.0.as_str()), Some("agent:held:more"), "the summary row is the top one: {shown:?}");
    let live: Vec<_> = shown.iter().filter(|r| r.0.starts_with("agent:held:") && r.0 != "agent:held:more" && r.2 == 1.0).collect();
    let places: Vec<_> = live.iter().map(|r| r.1.split('.').next().unwrap().to_string()).collect();
    assert_eq!(places, (1..=MAX_WAITING_SHOWN).map(|n| n.to_string()).collect::<Vec<_>>(), "in queue order: {shown:?}");
}

#[cfg(not(target_arch = "wasm32"))]
fn form_validation_harness(prepared: bool) -> (Harness, std::sync::Arc<solidcraft_engine::forms_prototype::validation::Input>) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.session.execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[1,1,1],"levels":0})).unwrap();
    h.app.apply_off_thread = Some(Duration::ZERO);
    h.app.preferences.agent_off_thread = false;
    let input = std::sync::Arc::new(solidcraft_engine::forms_prototype::validation::Input::capture(&h.app.session).unwrap());
    if prepared {
        crate::forms_prototype::install_validation_input_for_test(&mut h.app, input.clone());
    }
    (h, input)
}

#[cfg(not(target_arch = "wasm32"))]
fn request_form_validation(h: &mut Harness, method: &str, params: serde_json::Value) -> std::sync::mpsc::Receiver<serde_json::Value> {
    let (req, reply) = crate::ControlRequest::new(method, params);
    assert!(h.app.offload_control(&egui::Context::default(), req).is_none());
    reply
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_reads_pin_the_prepared_input_without_waiting_or_mutating_later_edits() {
    let (mut h, input) = form_validation_harness(true);
    let expected =
        serde_json::to_value(input.check(&solidcraft_engine::schema::forms_prototype::Validate { tolerance: Some(0.01) }).unwrap()).unwrap();
    h.app.apply_off_thread = Some(Duration::from_secs(10));
    let reply = request_form_validation(
        &mut h,
        "engine.execute",
        json!({"command":"form.prototype.validate","params":{"tolerance":0.01},"param_check":"strict"}),
    );
    assert_eq!(h.app.control_answering.len(), 1, "the special read never consumes within_wait, even if already completed");
    assert!(h.app.control_form_validation.is_empty());
    let p = h.app.session.forms_prototype.as_ref().unwrap().control_positions()[0];
    h.app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z+1.0]]})).unwrap();
    let edited = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
    h.app.finish_control_answers_for_test().unwrap();
    let answer = reply.recv().unwrap();
    assert_eq!(answer["ok"], true, "{answer}");
    assert_eq!(answer["param_check"], "strict");
    assert_eq!(answer["result"], expected, "an immutable old read is allowed to finish after a later edit");
    assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), edited);
    assert_eq!(h.app.session.param_check, solidcraft_engine::schema::ParamCheck::Off);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_waiting_preparation_refuses_changed_sources_documents_and_producer_errors() {
    for case in 0..3 {
        let (mut h, input) = form_validation_harness(false);
        let reply =
            request_form_validation(&mut h, "command", json!({"id":"form.prototype.validate","params":{"tolerance":0.01},"param_check":"warn"}));
        assert_eq!(h.app.control_form_validation.len(), 1);
        assert!(h.app.control_answering.is_empty());
        match case {
            0 => {
                let p = h.app.session.forms_prototype.as_ref().unwrap().control_positions()[0];
                h.app.session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[p.x,p.y,p.z+1.0]]})).unwrap();
            }
            1 => {
                h.app.session = solidcraft_engine::Session::default();
                h.app
                    .session
                    .execute("form.prototype.create", &json!({"primitive":"face","dimensions":[8,8,1],"segments":[1,1,1],"levels":0}))
                    .unwrap();
            }
            _ => {}
        }
        let stamp = solidcraft_engine::forms_prototype::validation::Stamp::current(&h.app.session).unwrap();
        let before = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
        h.app.advance_form_validation_for_test(Some(&stamp), Some(&input), (case == 2).then_some("producer stopped"));
        let answer = reply.recv().unwrap();
        assert_eq!(answer["ok"], false, "case {case}: {answer}");
        assert_eq!(answer["param_check"], "warn");
        assert!(answer["error"].as_str().unwrap().contains(if case == 2 { "producer stopped" } else { "source changed" }));
        assert!(h.app.control_form_validation.is_empty());
        assert!(h.app.control_answering.is_empty());
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), before);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_waiting_for_slots_keeps_the_original_ready_arc_and_ui_requests_answer() {
    let (mut h, input) = form_validation_harness(true);
    let expected =
        serde_json::to_value(input.check(&solidcraft_engine::schema::forms_prototype::Validate { tolerance: Some(0.01) }).unwrap()).unwrap();
    let held: Vec<_> = (0..4).map(|_| h.app.hold_form_answer_for_test(None)).collect();
    let count = std::sync::Arc::strong_count(&input);
    let reply = request_form_validation(&mut h, "engine.execute", json!({"command":"form.prototype.validate","params":{"tolerance":0.01}}));
    assert_eq!(h.app.control_answering.len(), 4);
    assert_eq!(h.app.control_form_validation.len(), 1);
    assert!(std::sync::Arc::strong_count(&input) > count, "the awaiting read owns its prepared source");
    let (tx, rx) = std::sync::mpsc::channel();
    h.app.control_rx = Some(rx);
    let inspect = ask(&tx, "ui.inspect", json!({}));
    h.frames(1);
    assert_eq!(inspect.try_recv().unwrap()["ok"], true, "inspection answers while all answer slots are held");
    assert_eq!(h.app.control_form_validation.len(), 1);
    h.app.session.execute("form.prototype.close", &json!({})).unwrap();
    let after = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
    for (done, _) in held {
        done.send(Some(json!({"ok":true}))).unwrap();
    }
    h.app.finish_control_answers_for_test().unwrap();
    h.app.advance_form_validation_for_test(None, None, None);
    assert_eq!(h.app.control_answering.len(), 1, "a captured read does not retarget or refuse a later document");
    h.app.finish_control_answers_for_test().unwrap();
    let answer = reply.recv().unwrap();
    assert_eq!(answer["result"], expected);
    assert!(answer.get("param_check").is_none());
    assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), after);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_awaiting_queue_is_bounded_and_cancelled_chats_never_launch() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let (mut h, input) = form_validation_harness(true);
    let held: Vec<_> = (0..4).map(|_| h.app.hold_form_answer_for_test(None)).collect();
    let turn = Arc::new(AtomicBool::new(true));
    let mut replies = Vec::new();
    for _ in 0..256 {
        let (mut req, reply) = crate::ControlRequest::new("engine.execute", json!({"command":"form.prototype.validate","params":{}}));
        req.chat_turn = Some(turn.clone());
        assert!(h.app.offload_control(&egui::Context::default(), req).is_none());
        replies.push(reply);
    }
    let overflow = request_form_validation(&mut h, "command", json!({"command":"form.prototype.validate","params":{},"param_check":"strict"}));
    let answer = overflow.recv().unwrap();
    assert_eq!(answer["ok"], false);
    assert_eq!(answer["param_check"], "strict");
    assert!(answer["error"].as_str().unwrap().contains("maximum 256"));
    assert_eq!(h.app.control_form_validation.len(), 256);
    assert_eq!(h.app.control_answering.len(), 4);
    turn.store(false, Ordering::Relaxed);
    let stamp = solidcraft_engine::forms_prototype::validation::Stamp::current(&h.app.session).unwrap();
    h.app.advance_form_validation_for_test(Some(&stamp), Some(&input), None);
    assert!(h.app.control_form_validation.is_empty());
    for reply in replies {
        assert_eq!(reply.recv().unwrap(), crate::control::cancelled_chat_response());
    }
    assert_eq!(h.app.control_answering.len(), 4, "none of the dropped chat reads starts a worker");
    for (done, _) in held {
        done.send(Some(json!({"ok":true}))).unwrap();
    }
    h.app.finish_control_answers_for_test().unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_ticket_cancellation_drops_a_waiting_read() {
    let (mut h, input) = form_validation_harness(true);
    let held: Vec<_> = (0..4).map(|_| h.app.hold_form_answer_for_test(None)).collect();
    let (req, reply) = crate::ControlRequest::new("engine.execute", json!({"command":"form.prototype.validate","params":{}}));
    let ticket = req.ticket.clone();
    assert!(h.app.offload_control(&egui::Context::default(), req).is_none());
    assert_eq!(h.app.control_form_validation.len(), 1);

    assert_eq!(ticket.cancel(), crate::control_job::Cancel::Stopped { writes_file: false });
    let stamp = solidcraft_engine::forms_prototype::validation::Stamp::current(&h.app.session).unwrap();
    h.app.advance_form_validation_for_test(Some(&stamp), Some(&input), None);
    assert!(h.app.control_form_validation.is_empty());
    assert_eq!(h.app.control_answering.len(), 4);
    let cancelled = reply.try_recv().unwrap();
    assert_eq!(cancelled["stopped"], true);
    assert!(!cancelled["error"].as_str().unwrap().contains("file"));

    for (done, _) in held {
        done.send(Some(json!({"ok":true}))).unwrap();
    }
    h.app.finish_control_answers_for_test().unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_form_cad_control_workers_drain_before_releasing_their_answer_slots() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let (mut h, _) = form_validation_harness(true);
    let turn = Arc::new(AtomicBool::new(true));
    let held: Vec<_> = (0..4).map(|_| h.app.hold_form_answer_for_test(Some(turn.clone()))).collect();
    turn.store(false, Ordering::Relaxed);
    let ctx = egui::Context::default();
    h.app.poll_control_jobs(&ctx);
    for (_, reply) in &held {
        assert_eq!(reply.recv().unwrap(), crate::control::cancelled_chat_response());
    }
    h.app.poll_control_jobs(&ctx);
    for (_, reply) in &held {
        assert!(reply.try_recv().is_err(), "a chat cancellation is answered once");
    }
    assert_eq!(h.app.control_answering.len(), 4, "cancelled CPU jobs still own the four slots");
    let reply = request_form_validation(&mut h, "engine.execute", json!({"command":"form.prototype.validate","params":{"tolerance":0.01}}));
    assert_eq!(h.app.control_form_validation.len(), 1);
    for (done, _) in held {
        done.send(Some(json!({"ok":true}))).unwrap();
    }
    h.app.poll_control_jobs(&ctx);
    assert!(h.app.control_form_validation.is_empty());
    assert_eq!(h.app.control_answering.len(), 1);
    h.app.finish_control_answers_for_test().unwrap();
    assert_eq!(reply.recv().unwrap()["ok"], true);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_spawn_failure_and_scope_metadata_errors_never_run_synchronously() {
    let (mut h, _) = form_validation_harness(true);
    let held: Vec<_> = (0..4).map(|_| h.app.hold_form_answer_for_test(None)).collect();
    let before = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
    let root = h.dir.to_string_lossy().into_owned();
    let reply = request_form_validation(
        &mut h,
        "command",
        json!({"command":"form.prototype.validate","params":{},"allow_paths":[root],"param_check":"warn"}),
    );
    for (done, _) in held {
        done.send(Some(json!({"ok":true}))).unwrap();
    }
    h.app.finish_control_answers_for_test().unwrap();
    let scope = h.app.fail_form_validation_spawn_for_test().unwrap().unwrap();
    assert!(scope.check(&root).is_ok(), "the original validated folder scope reaches the launcher");
    let answer = reply.recv().unwrap();
    assert_eq!(answer["ok"], false);
    assert_eq!(answer["param_check"], "warn");
    assert!(answer["error"].as_str().unwrap().contains("could not start"));
    assert!(h.app.control_answering.is_empty());
    assert!(h.app.control_form_validation.is_empty());
    for (params, text) in [
        (json!({"command":"form.prototype.validate","params":{},"allow_paths":false,"param_check":"strict"}), "allow_paths"),
        (json!({"command":"form.prototype.validate","params":{},"allow_paths":false,"param_check":"loud"}), "param_check"),
    ] {
        let reply = request_form_validation(&mut h, "engine.execute", params).recv().unwrap();
        assert_eq!(reply["ok"], false);
        assert!(reply["error"].as_str().unwrap().contains(text));
        assert!(reply.get("param_check").is_none(), "metadata errors match control::handle before dispatch_checked");
    }
    assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), before);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_checks_keep_canonical_schema_levels_and_headless_compatibility() {
    use solidcraft_engine::schema::ParamCheck;
    for (level, params) in [
        (ParamCheck::Strict, json!({"tolerance":0.01})),
        (ParamCheck::Strict, json!({"tolerence":0.01})),
        (ParamCheck::Warn, json!({"tolerence":0.01})),
        (ParamCheck::Off, json!({"tolerance":"invalid"})),
    ] {
        let (mut h, _) = form_validation_harness(true);
        h.app.session.param_check = level;
        let canonical = h.app.session.execute("form.prototype.validate", &params);
        let before = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
        let reply = request_form_validation(
            &mut h,
            "engine.execute",
            json!({"command":"form.prototype.validate","params":params,"param_check":level.name()}),
        );
        h.app.finish_control_answers_for_test().unwrap();
        let answer = reply.recv().unwrap();
        assert_eq!(answer["param_check"], level.name());
        match canonical {
            Ok(result) => {
                assert_eq!(answer["ok"], true);
                assert_eq!(answer["result"], result);
            }
            Err(error) => {
                assert_eq!(answer["ok"], false);
                assert_eq!(answer["error"], error.to_string());
            }
        }
        assert_eq!(h.app.session.param_check, level);
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), before);
    }
    let (mut h, _) = form_validation_harness(false);
    h.app.apply_off_thread = None;
    let (req, reply) = crate::ControlRequest::new("engine.execute", json!({"command":"form.prototype.validate","params":{"tolerance":0.01}}));
    let req = h.app.offload_control(&egui::Context::default(), req).expect("headless callers keep the existing direct execution path");
    h.app.run_control(&egui::Context::default(), req);
    assert_eq!(reply.recv().unwrap()["ok"], true);
    assert!(h.app.control_form_validation.is_empty());
    assert!(h.app.control_answering.is_empty());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn form_cad_control_honours_the_scoped_host_check_once_and_restores_effective_parameters() {
    use solidcraft_engine::schema::ParamCheck;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    for panic in [false, true] {
        let (mut h, _) = form_validation_harness(true);
        let root = h.dir.to_string_lossy().into_owned();
        let folder = root.clone();
        let uid = h.app.session.uid;
        let source = h.app.session.forms_prototype.as_ref().unwrap().source_revision();
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        h.app.session.set_command_check(Some(Arc::new(move |id, params, session| {
            called.fetch_add(1, Ordering::Relaxed);
            assert_eq!(id, "form.prototype.validate");
            assert_eq!(params, &json!({"tolerance":0.01}));
            assert_eq!(session.uid, uid);
            assert_eq!(session.forms_prototype.as_ref().unwrap().source_revision(), source);
            assert_eq!(session.param_check, ParamCheck::Strict);
            assert!(solidcraft_engine::io::vfs::current_scope().unwrap().check(&folder).is_ok());
            if panic {
                panic!("deliberate host query check panic");
            }
            Err("host policy refused this CAD check".into())
        })));
        let before = solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap();
        let answer = request_form_validation(
            &mut h,
            "engine.execute",
            json!({"command":"form.prototype.validate","params":{"tolerance":0.01},"allow_paths":[root],"param_check":"strict"}),
        )
        .recv()
        .unwrap();
        assert_eq!(answer["ok"], false);
        assert_eq!(answer["param_check"], "strict");
        assert!(answer["error"].as_str().unwrap().contains(if panic { "host's command check failed" } else { "host policy refused" }));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(h.app.session.param_check, ParamCheck::Off);
        assert!(h.app.control_answering.is_empty());
        assert!(h.app.control_form_validation.is_empty());
        assert!(solidcraft_engine::io::vfs::current_scope().is_none());
        assert_eq!(solidcraft_engine::recovery::capture_bytes(&h.app.session).unwrap(), before);
    }
}
