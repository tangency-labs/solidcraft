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
    // The window's own operation check would hand OK its Join already evaluated (#6901): these
    // tests are about OK computing it.
    if let Some(d) = h.app.dialog.as_mut() {
        d.op_auto.manual = true;
    }
    h.app.op_check = Default::default();
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

/// An evaluated result held behind a channel, so widget assertions do not race the worker.
fn hold_result(h: &mut Harness, different_commands: bool) -> std::sync::mpsc::Sender<Result<solidcraft_engine::ComputedCommands, String>> {
    let cmds = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().unwrap()).unwrap();
    let (finish, done) = std::sync::mpsc::channel();
    h.app.applying = Some(crate::dialogs::Applying {
        done,
        ready: None,
        seen: true,
        started: crate::now_ms(),
        cmds: if different_commands { Vec::new() } else { cmds },
        progress: Default::default(),
        quality: Default::default(),
        design: crate::dialogs::Design::of(&h.app.session),
    });
    finish
}

fn result(h: &Harness) -> Result<solidcraft_engine::ComputedCommands, String> {
    let dialog = h.app.dialog.as_ref().unwrap();
    let commands = crate::dialogs::apply_commands(&h.app, dialog).unwrap();
    match &h.app.editing_state {
        Some(state) => h.app.session.scratch().compute_editor_commands(&commands, &state.source, state.original.scratch()),
        None => h.app.session.scratch().compute_commands(&commands, dialog.editing.map(|(_, marker)| marker)),
    }
    .map_err(|e| e.to_string())
}

#[test]
fn ok_computes_off_the_window_thread_saying_so_then_applies() {
    let mut h = square_on_a_block();
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    h.frames(1);
    assert!(h.widget("Computing\u{2026} 0 s").is_some(), "the dialog says it computes");
    h.step(&json!({"key": "Enter"})).unwrap();
    finish.send(prepared).ok().unwrap();
    let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    solidcraft_engine::doc::progress::watch(window.clone(), || until_applied(&mut h));
    assert_eq!(window.features(), (0, 0), "adoption does not evaluate the model");
    assert!(h.app.dialog.is_none());
    h.step(&json!({"expect": {"bodies": 1, "volume": 13000}})).unwrap();
    assert_eq!(h.app.session.doc.features.len(), 4);
}

#[test]
fn cancel_while_computing_applies_nothing() {
    let mut h = square_on_a_block();
    let prepared = result(&h);
    let features = h.app.session.doc.features.len();
    let finish = hold_result(&mut h, false);
    let progress = h.app.applying.as_ref().unwrap().progress.clone();
    h.step(&json!({"key": "Escape"})).unwrap();
    assert!(h.app.applying.is_none() && h.app.dialog.is_none());
    // Cancel leaves the evaluation running for OK to take up again (#3962), and applies nothing.
    assert!(!progress.is_cancelled() && h.app.abandoned.len() == 1);
    finish.send(prepared).ok().unwrap();
    h.frames(5);
    assert_eq!(h.app.session.doc.features.len(), features);
}

#[test]
fn removing_the_edited_feature_discards_its_pending_result() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.run("solid.box", json!({"length":10,"width":10,"height":10,"name":"Block"})).unwrap();
    h.call("ui.editFeature", json!({"feature":"Block"}));
    h.frames(2);
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    let progress = h.app.applying.as_ref().unwrap().progress.clone();
    h.app.run("edit.undo", json!({})).unwrap();
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    h.frames(2);
    assert!(h.app.dialog.is_none() && h.app.applying.is_none() && h.app.editing_state.is_none());
    assert!(progress.is_cancelled());
    assert!(finish.send(prepared).is_err(), "the removed editor cannot adopt a late result");
    h.frames(2);
    assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
}

#[test]
fn a_ready_transaction_waits_one_frame_before_adoption() {
    let mut h = square_on_a_block();
    let features = h.app.session.doc.features.len();
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    h.app.applying.as_mut().unwrap().seen = false;
    finish.send(prepared).ok().unwrap();
    h.frames(1);
    assert_eq!(h.app.session.doc.features.len(), features);
    assert!(h.app.applying.is_some());
    until_applied(&mut h);
    assert_eq!(h.app.session.doc.features.len(), features + 1);
}

#[test]
fn restarting_the_same_command_discards_the_previous_dialog_job() {
    let mut h = square_on_a_block();
    let document = h.app.session.doc.clone();
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    let progress = h.app.applying.as_ref().unwrap().progress.clone();
    h.app.start("solid.extrude");
    assert!(h.app.applying.is_none());
    assert!(h.app.dialog.is_some());
    assert!(progress.is_cancelled());
    assert!(finish.send(prepared).is_err());
    h.frames(2);
    assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
}

#[test]
fn switching_tabs_cancels_the_transaction_without_changing_either_document() {
    let mut h = square_on_a_block();
    let original = h.app.session.doc.clone();
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    let progress = h.app.applying.as_ref().unwrap().progress.clone();
    let other = solidcraft_engine::Session::default();
    let other_doc = other.doc.clone();
    crate::documents::open_tab(&mut h.app, other).unwrap();
    assert!(progress.is_cancelled());
    assert!(finish.send(prepared).is_err());
    assert!(h.app.applying.is_none() && h.app.dialog.is_none() && h.app.editing_state.is_none());
    h.frames(2);
    assert!(std::sync::Arc::ptr_eq(&other_doc, &h.app.session.doc));
    crate::documents::switch(&mut h.app, 0);
    assert!(std::sync::Arc::ptr_eq(&original, &h.app.session.doc));
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
    let picked = h.app.dialog.as_ref().unwrap().inputs[0].items.clone();
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    h.step(&json!({"click": {"world": [10, 10, 10]}})).unwrap();
    h.frames(2);
    assert_eq!(h.app.dialog.as_ref().unwrap().inputs[0].items, picked);
    finish.send(prepared).ok().unwrap();
    until_applied(&mut h);
    assert!(h.app.dialog.is_none());
    h.step(&json!({"expect": {"bodies": 1, "volume": 13000}})).unwrap();
}

#[test]
fn inputs_changed_while_ok_computes_are_computed_off_the_thread_too() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    let features = h.app.session.doc.features.len();
    let prepared = result(&h);
    let finish = hold_result(&mut h, true);
    finish.send(prepared).ok().unwrap();
    let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    solidcraft_engine::doc::progress::watch(window.clone(), || until_applied(&mut h));
    assert_eq!(window.features(), (0, 0));
    assert!(h.app.dialog.is_none());
    assert_eq!(h.app.session.doc.features.len(), features + 1);
    h.step(&json!({"expect": {"bodies": 1, "volume": 13000}})).unwrap();
}

#[test]
fn changed_document_rejects_a_completed_result() {
    let mut h = square_on_a_block();
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    h.app.run("solid.box", json!({"length":2,"width":2,"height":2})).unwrap();
    let current = h.app.session.doc.clone();
    finish.send(prepared).ok().unwrap();
    until_applied(&mut h);
    assert!(std::sync::Arc::ptr_eq(&current, &h.app.session.doc));
    assert!(h.app.dialog.as_ref().unwrap().error.is_some());
}

#[test]
fn hole_ok_adopts_real_cut_without_window_evaluation() {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.set_origin_shown(false);
    h.app.apply_off_thread = Some(Duration::ZERO);
    h.app.run("solid.box", json!({"length":40,"width":30,"height":20})).unwrap();
    h.call("ui.view", json!({"view":"top"}));
    h.app.fit_view();
    h.settle();
    let undo = h.app.session.undo.len();
    h.step(&json!({"start":"solid.hole"})).unwrap();
    h.step(&json!({"click":{"world":[12,9,20]}})).unwrap();
    h.step(&json!({"click":{"handle":"dialog:Diameter"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"4 mm"})).unwrap();
    let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    solidcraft_engine::doc::progress::watch(window.clone(), || {
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        until_applied(&mut h);
    });
    assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|d| d.error.clone()));
    assert_eq!(window.features(), (0, 0));
    assert_eq!(h.app.session.undo.len(), undo + 1);
    let volume = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!(volume < 23990.0 && volume > 23000.0, "the hole removed actual material: {volume}");
    h.app.run("edit.undo", json!({})).unwrap();
    h.step(&json!({"expect":{"volume":24000,"bodies":1}})).unwrap();
}

fn cut_chain() -> (Harness, u64) {
    let mut h = Harness::new();
    h.app.home.open = false;
    h.app.apply_off_thread = Some(std::time::Duration::ZERO);
    h.app.run("solid.box", json!({"length":80,"width":50,"height":20,"body_name":"Stock"})).unwrap();
    let mut target = 0;
    for index in 0..6 {
        let sketch = format!("Circle{}", index + 1);
        h.app.run("sketch.create", json!({"plane":"XY","offset":20,"name":sketch})).unwrap();
        h.app.run("sketch.circle.center", json!({"center":[10 + 10 * index,25],"diameter":4})).unwrap();
        h.app.run("sketch.finish", json!({})).unwrap();
        let result = h
            .app
            .run(
                "solid.extrude",
                json!({"sketch":sketch,"profiles":"all","distance":6,"direction":"negative","operation":"cut","name":format!("Cut{}", index+1)}),
            )
            .unwrap();
        if index == 0 {
            target = result["feature"].as_u64().unwrap();
        }
    }
    (h, target)
}

#[test]
fn changed_first_cut_adopts_its_entire_suffix_without_window_evaluation() {
    let (mut h, target) = cut_chain();
    let before = h.app.run("inspect.measure", json!({})).unwrap();
    let undo = h.app.session.undo.len();
    h.call("ui.editFeature", json!({"feature":target}));
    h.frames(3);
    assert!(h.app.session.feature_edit_sketch.is_some(), "the edited Extrude keeps its input sketch visible");
    h.step(&json!({"click":{"handle":"dialog:Distance"}})).unwrap();
    h.step(&json!({"key":"A","cmd":true})).unwrap();
    h.step(&json!({"text":"9 mm"})).unwrap();
    let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    solidcraft_engine::doc::progress::watch(window.clone(), || {
        h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
        for _ in 0..5000 {
            h.frames(1);
            if h.app.dialog.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });
    assert_eq!(window.features(), (0, 0), "the complete changed suffix is adopted without UI evaluation");
    assert!(h.app.dialog.is_none(), "changed edit did not complete");
    assert_eq!(h.app.session.feature_edit_sketch, None, "Apply clears the temporary sketch override");
    let after = h.app.run("inspect.measure", json!({})).unwrap();
    let before_volume = before["total"]["volume_mm3"].as_f64().unwrap();
    let after_volume = after["total"]["volume_mm3"].as_f64().unwrap();
    assert!(before_volume - after_volume > 30.0, "the edit must change actual geometry: {before_volume} -> {after_volume}");
    assert_eq!(h.app.session.undo.len(), undo + 1);
    assert_eq!(h.app.session.doc.marker, None);
    h.app.run("edit.undo", json!({})).unwrap();
    let restored = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((restored - before_volume).abs() < 0.001);
    h.app.run("edit.redo", json!({})).unwrap();
    let redone = h.app.run("inspect.measure", json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((redone - after_volume).abs() < 0.001);
}

#[test]
fn cancelling_an_edit_restores_pinned_geometry_without_evaluating_an_evicted_suffix() {
    let (mut h, target) = cut_chain();
    h.app.run("solid.box", json!({"corner":[100,0,0],"length":2,"width":2,"height":2})).unwrap();
    h.app.run("edit.undo", json!({})).unwrap();
    let before = h.app.session.world_state();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    let visibility = h.app.session.visibility.clone();
    h.call("ui.editFeature", json!({"feature":target}));
    crate::dialogs::sync_feature_edit(&mut h.app);
    let sketch = h.app.session.feature_edit_sketch.expect("edited Extrude exposes its sketch");
    assert!(solidcraft_engine::view::sketch_shown(&h.app.session, sketch));
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    *h.app.session.model.cache.lock().unwrap() = Default::default();
    // Picks are view state: cancellation keeps a newer selection.
    h.app.session.selection = vec![solidcraft_engine::Sel::Axis { name: "X".into() }];
    let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    solidcraft_engine::doc::progress::watch(window.clone(), || {
        h.step(&json!({"key":"Escape"})).unwrap();
    });
    assert_eq!(window.features(), (0, 0), "Cancel never rebuilds the old suffix on the window thread");
    assert!(h.app.dialog.is_none() && h.app.applying.is_none() && h.app.editing_state.is_none());
    assert!(finish.send(prepared).is_err());
    assert_eq!(h.app.session.feature_edit_sketch, None, "Cancel clears the temporary sketch override");
    assert_eq!(h.app.session.visibility, visibility, "temporary sketch display does not change stored visibility");
    assert_eq!(h.app.session.doc.marker, None);
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    assert!(std::sync::Arc::ptr_eq(&before, &h.app.session.world_state()));
    assert_eq!(h.app.session.selection, vec![solidcraft_engine::Sel::Axis { name: "X".into() }]);
}

#[test]
fn replacing_a_document_discards_pending_apply_and_editor_backup() {
    let (mut h, target) = cut_chain();
    h.call("ui.editFeature", json!({"feature":target}));
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    h.app.run("file.new", json!({})).unwrap();
    assert!(h.app.dialog.is_none() && h.app.applying.is_none() && h.app.editing_state.is_none());
    assert!(finish.send(prepared).is_err());
    h.frames(3);
    assert!(h.app.session.doc.features.is_empty());
}

#[test]
fn cancelling_a_stale_editor_preserves_the_newer_document() {
    let (mut h, target) = cut_chain();
    h.call("ui.editFeature", json!({"feature":target}));
    h.app.session.execute("solid.box", &json!({"length":2,"width":2,"height":2})).unwrap();
    let document = h.app.session.doc.clone();
    h.step(&json!({"key":"Escape"})).unwrap();
    assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(h.app.dialog.is_none());
    assert!(h.app.status.as_ref().is_some_and(|(_, _, error)| *error));
}

#[test]
fn hiding_an_origin_plane_during_feature_edit_allows_ok_and_cancel() {
    for apply in [false, true] {
        let mut h = Harness::new();
        h.app.home.open = false;
        for (command, params) in [
            ("sketch.create", json!({"plane":"XY"})),
            ("sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[30,20]})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"distance":10,"operation":"new"})),
        ] {
            h.app.run(command, params).unwrap();
        }
        h.call("ui.editFeature", json!({"feature":"Extrude1"}));
        h.frames(2);
        h.step(&json!({"click":{"handle":"dialog:Distance"}})).unwrap();
        h.step(&json!({"key":"A","cmd":true})).unwrap();
        h.step(&json!({"text":"20 mm"})).unwrap();
        h.step(&json!({"click":{"handle":"fold:Origin"}})).unwrap();
        h.step(&json!({"click":{"handle":"eye:XY"}})).unwrap();
        assert!(h.app.session.visibility.hidden_origin.iter().any(|name| name == "XY"));
        if apply {
            h.step(&json!({"click":{"handle":"dialog:OK"}})).unwrap();
            until_applied(&mut h);
        } else {
            h.step(&json!({"key":"Escape"})).unwrap();
        }
        assert!(h.app.dialog.is_none(), "{:?}", h.app.dialog.as_ref().and_then(|dialog| dialog.error.as_ref()));
        assert!(h.app.status.as_ref().is_none_or(|(_, _, error)| !error), "{:?}", h.app.status);
        assert_eq!(h.app.session.doc.marker, None);
        assert!(h.app.session.visibility.hidden_origin.iter().any(|name| name == "XY"));
        h.step(&json!({"expect":{"bodies":1,"volume":if apply {12000} else {6000}}})).unwrap();
        if apply {
            h.app.run("edit.undo", json!({})).unwrap();
            assert_eq!(h.app.session.doc.marker, None);
            assert!(h.app.session.visibility.hidden_origin.iter().any(|name| name == "XY"));
        }
        h.app.run("edit.undo", json!({})).unwrap();
        assert_eq!(h.app.session.doc.marker, None, "undoing the eye click keeps the complete timeline");
        assert!(!h.app.session.visibility.hidden_origin.iter().any(|name| name == "XY"));
        h.step(&json!({"expect":{"bodies":1,"volume":6000}})).unwrap();
    }
}

#[test]
fn a_worker_error_then_cancel_preserves_original_world_history_redo_and_selection() {
    let (mut h, target) = cut_chain();
    h.app.run("solid.box", json!({"corner":[100,0,0],"length":2,"width":2,"height":2})).unwrap();
    h.app.run("edit.undo", json!({})).unwrap();
    let world = h.app.session.world_state();
    let document = h.app.session.doc.clone();
    let history = (h.app.session.undo.len(), h.app.session.redo.len());
    h.app.session.selection = vec![solidcraft_engine::Sel::Feature { id: target }];
    let selection = h.app.session.selection.clone();
    h.call("ui.editFeature", json!({"feature":target}));
    let prefix = h.app.session.doc.clone();
    let dialog = h.app.dialog.as_mut().unwrap();
    if let crate::dialogs::Kind::Extrude { distance, .. } = &mut dialog.kind {
        *distance = "missing_extent".into();
    }
    let dialog = dialog.clone();
    let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    solidcraft_engine::doc::progress::watch(window.clone(), || {
        // Exercise the production worker directly: a preview may disable the OK widget
        // for this deliberately invalid expression before a synthetic click reaches it.
        h.app.applying = crate::dialogs::begin_applying(&mut h.app, &dialog);
        assert!(h.app.applying.is_some(), "the real apply worker was started");
        until_applied(&mut h);
        assert!(h.app.dialog.as_ref().and_then(|d| d.error.as_ref()).is_some_and(|error| error.contains("missing_extent")), "{:?}", h.app.dialog);
        assert!(std::sync::Arc::ptr_eq(&prefix, &h.app.session.doc), "worker failure leaves the live prefix untouched");
        h.step(&json!({"key":"Escape"})).unwrap();
    });
    assert_eq!(window.features(), (0, 0));
    assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(std::sync::Arc::ptr_eq(&world, &h.app.session.world_state()));
    assert_eq!((h.app.session.undo.len(), h.app.session.redo.len()), history);
    assert_eq!(h.app.session.selection, selection);
}

#[test]
fn another_interactive_command_closes_the_editor_without_rebuilding_its_old_suffix() {
    let (mut h, target) = cut_chain();
    let document = h.app.session.doc.clone();
    h.call("ui.editFeature", json!({"feature":target}));
    let window = std::sync::Arc::new(solidcraft_engine::doc::progress::Progress::default());
    solidcraft_engine::doc::progress::watch(window.clone(), || h.app.start("solid.hole"));
    assert_eq!(window.features(), (0, 0));
    assert!(std::sync::Arc::ptr_eq(&document, &h.app.session.doc));
    assert!(h.app.editing_state.is_none());
    assert!(h.app.dialog.as_ref().is_some_and(|d| d.editing.is_none() && matches!(d.kind, crate::dialogs::Kind::Hole { .. })));
}

#[test]
fn a_changed_display_quality_warms_the_result_without_replaying_commands() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    let prepared = result(&h);
    let finish = hold_result(&mut h, false);
    h.app.preferences.display_quality = "fine".into();
    finish.send(prepared).ok().unwrap();
    h.frames(1);
    let progress = h.app.applying.as_ref().expect("the new display plans run off the window thread").progress.clone();
    until_applied(&mut h);
    assert_eq!(progress.features(), (0, 0), "changing display quality does not replay the geometry commands");
    assert!(h.app.dialog.is_none());
    h.step(&json!({"expect":{"bodies":1,"volume":13000}})).unwrap();
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

/// #3962: an abandoned job computed for a different design is never taken up, even when the
/// dialog's commands are identical.
#[test]
fn ok_again_does_not_take_up_a_job_computed_for_a_different_design() {
    let mut h = square_on_a_block();
    h.app.apply_off_thread = Some(Duration::ZERO);
    let (_finish, gate) = std::sync::mpsc::channel();
    h.app.apply_gate = Some(std::sync::Arc::new(std::sync::Mutex::new(gate)));
    h.step(&json!({"key": "Enter"})).unwrap();
    h.step(&json!({"key": "Escape"})).unwrap();
    assert_eq!(h.app.abandoned.len(), 1, "left computing");
    h.app.run("solid.box", json!({"corner": [100, 0, 0], "length": 2, "width": 2, "height": 2})).unwrap();
    h.app.start("solid.extrude");
    h.frames(2);
    h.step(&json!({"key": "Enter"})).unwrap();
    assert!(h.app.applying.is_some(), "a new evaluation for the changed design");
    assert_eq!(h.app.abandoned.len(), 1, "the other design's job was left alone");
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
