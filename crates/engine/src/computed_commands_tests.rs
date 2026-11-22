//! Prepared transactions preserve live state until accepted and reject stale sources.
use crate::Session;
use serde_json::json;
use std::sync::Arc;

fn commands() -> Vec<(String, serde_json::Value)> {
    vec![("solid.box".into(), json!({"length":2,"width":3,"height":4}))]
}

#[test]
fn acceptance_preserves_file_history_and_prepared_world_without_evaluation() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
    session.path = Some("working.solidcraft".into());
    let saved = session.saved.clone();
    let checkpoint = session.checkpoint();
    let history = session.undo.len();
    let prepared = session.scratch().compute_commands(&commands(), None).unwrap();
    prepared.warm_meshes(|| true).unwrap();
    let world = prepared.computed.world_state().unwrap();
    let before = session.evaluations;
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.evaluations, before);
    assert_eq!(session.path.as_deref(), Some("working.solidcraft"));
    assert!(Arc::ptr_eq(&saved, &session.saved));
    assert_eq!(session.undo.len(), history + 1);
    assert!(Arc::ptr_eq(&world, &session.world_state().unwrap()));
    session.restore_checkpoint(checkpoint).unwrap();
    assert_eq!(session.undo.len(), history);
    assert_eq!(session.doc.features.len(), 1);
}

#[test]
fn prepared_preview_includes_visibility_and_command_replies() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":1,"width":1,"height":1,"body_name":"Hidden"})).unwrap();
    session.execute("browser.visibility", &json!({"items":["Hidden"],"visible":false})).unwrap();
    let history = session.undo.len();
    let document = session.doc.clone();
    let prepared = session.scratch().compute_commands(&commands(), None).unwrap();

    let preview = prepared.preview_visible_world().unwrap();
    assert!(preview.before.bodies.is_empty());
    assert_eq!(preview.after.bodies.len(), 1);
    assert_eq!(preview.replies.as_slice(), std::slice::from_ref(&prepared.steps[0].reply));
    assert!(prepared.changes_document());
    assert!(Arc::ptr_eq(&document, &session.doc));
    assert_eq!(session.undo.len(), history);
}

#[test]
fn an_equal_document_in_a_new_allocation_is_unchanged() {
    let session = Session::default();
    let mut prepared = session.scratch().compute_commands(&[], None).unwrap();
    prepared.computed.doc = Arc::new((*prepared.computed.doc).clone());
    assert!(!Arc::ptr_eq(&prepared.source.doc, &prepared.computed.doc));
    assert!(!prepared.changes_document());
}

#[test]
fn failure_discards_the_entire_batch() {
    let session = Session::default();
    let document = session.doc.clone();
    let mut batch = commands();
    batch.push(("solid.box".into(), json!({"length":-1})));
    assert!(session.scratch().compute_commands(&batch, None).is_err());
    assert!(Arc::ptr_eq(&document, &session.doc));
    assert!(session.undo.is_empty());
}

#[test]
fn revision_context_selection_and_replaced_document_reject_old_work() {
    for change in 0..5 {
        let mut session = Session::default();
        let prepared = session.scratch().compute_commands(&commands(), None).unwrap();
        match change {
            0 => session.revision += 1,
            1 => session.active_component = 17,
            2 => session.active_occurrence_path = Some(vec![1]),
            3 => session.selection.push(crate::Sel::Axis { name: "X".into() }),
            _ => session = Session::default(),
        }
        let document = session.doc.clone();
        assert!(!prepared.is_current(&session));
        assert!(session.accept_commands(prepared).is_err());
        assert!(Arc::ptr_eq(&document, &session.doc));
        assert!(session.undo.is_empty());
    }
}

#[test]
fn editor_marker_restoration_reuses_the_pinned_model_with_an_empty_cache() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
    let world = session.world_state().unwrap();
    let original = session.scratch();
    session.execute("timeline.roll_to", &json!({"position":0})).unwrap();
    let source = session.scratch();
    *session.model.cache.lock().unwrap() = Default::default();
    let prepared = session.scratch().compute_editor_commands(&[], &source, original).unwrap();
    assert_eq!(prepared.computed.evaluations, 0, "restoring the marker must never evaluate the old suffix");
    assert!(Arc::ptr_eq(&world, &prepared.computed.world_state().unwrap()));
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.doc.marker, None);
}

#[test]
fn an_editor_cannot_take_a_pinned_model_from_another_document() {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"length":2,"width":3,"height":4})).unwrap();
    session.execute("timeline.roll_to", &json!({"position":0})).unwrap();
    let source = session.scratch();
    let document = session.doc.clone();
    assert!(session.scratch().compute_editor_commands(&[], &source, Session::default()).is_err());
    assert!(Arc::ptr_eq(&document, &session.doc));
}

fn extrusion_editor() -> (Session, Session, Session, Vec<(String, serde_json::Value)>) {
    let mut session = Session::default();
    session.execute("solid.box", &json!({"corner":[-10,0,0],"length":2,"width":3,"height":4,"body_name":"Other"})).unwrap();
    let sketch = session.execute("sketch.create", &json!({"plane":"XY"})).unwrap()["sketch"].as_u64().unwrap();
    session.execute("sketch.rectangle.two_point", &json!({"p0":[0,0],"p1":[30,20]})).unwrap();
    session.execute("sketch.finish", &json!({})).unwrap();
    let feature = session.execute("solid.extrude", &json!({"distance":10,"operation":"new"})).unwrap()["feature"].as_u64().unwrap();
    let original = session.scratch();
    // The UI's temporary rollback does not create an undo step.
    let undo = std::mem::take(&mut session.undo);
    session.execute("timeline.roll_to", &json!({"position":2})).unwrap();
    session.undo = undo;
    let source = session.scratch();
    let commands = vec![(
        "timeline.redefine".into(),
        json!({"feature":feature,"command":"solid.extrude","params":{"sketch":sketch,"profiles":"all","distance":20,"operation":"new"}}),
    )];
    (session, source, original, commands)
}

fn assert_editor_volume(session: &mut Session, volume: f64) {
    assert_eq!(session.doc.marker, None, "visibility history must not restore the editor's temporary rollback");
    let actual = session.execute("inspect.measure", &json!({})).unwrap()["total"]["volume_mm3"].as_f64().unwrap();
    assert!((actual - volume).abs() < 1e-6, "expected {volume}, got {actual}");
}

#[test]
fn editor_visibility_changes_allow_apply_and_preserve_separate_undo_steps() {
    for params in [
        json!({"items":["Other"],"visible":false}),
        json!({"items":["XY"],"visible":false}),
        json!({"items":["Sketch1"],"visible":false}),
        json!({"tree_folders":["c0/bodies"],"visible":false}),
    ] {
        let (mut session, source, original, commands) = extrusion_editor();
        let shown = session.visibility.clone();
        let history = session.undo.len();
        session.execute("browser.visibility", &params).unwrap();
        let hidden = session.visibility.clone();
        assert_ne!(hidden, shown, "{params}");
        assert!(Arc::ptr_eq(&source.doc, &session.doc));
        let prepared = session.scratch().compute_editor_commands(&commands, &source, original).unwrap();
        session.accept_commands(prepared).unwrap();
        assert_eq!(session.undo.len(), history + 2, "hide and edit are separate undo steps");
        assert_eq!(session.visibility, hidden);
        assert_editor_volume(&mut session, 12024.0);
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(session.visibility, hidden);
        assert_editor_volume(&mut session, 6024.0);
        session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(session.visibility, shown);
        assert_editor_volume(&mut session, 6024.0);
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.visibility, hidden);
        assert_editor_volume(&mut session, 6024.0);
        session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(session.visibility, hidden);
        assert_editor_volume(&mut session, 12024.0);
    }
}

/// A section, an analysis or the selection changed while an editor is open changes only what is
/// shown: OK still applies the edit and Cancel puts the design back (#10687).
#[test]
fn editor_display_changes_allow_apply_and_cancel() {
    let cases: [&[(&str, serde_json::Value)]; 5] = [
        &[("inspect.section", json!({"plane":"XZ","offset":1}))],
        &[("inspect.section", json!({"plane":"XZ","offset":1})), ("inspect.section", json!({"clear":true}))],
        &[("inspect.zebra", json!({"stripes":14.5}))],
        &[("select.set", json!({"items":[{"type":"body","name":"Other"}]}))],
        &[("select.clear", json!({}))],
    ];
    for display in cases {
        let (mut session, source, original, commands) = extrusion_editor();
        for (id, params) in display {
            session.execute(id, params).unwrap();
        }
        let prepared =
            session.scratch().compute_editor_commands(&commands, &source, original).unwrap_or_else(|e| panic!("OK after {display:?}: {e}"));
        session.accept_commands(prepared).unwrap();
        assert_editor_volume(&mut session, 12024.0);
        let (mut session, source, original, _) = extrusion_editor();
        for (id, params) in display {
            session.execute(id, params).unwrap();
        }
        session.restore_editor(&source, original).unwrap_or_else(|e| panic!("Cancel after {display:?}: {e}"));
        assert_editor_volume(&mut session, 6024.0);
    }
}

#[test]
fn cancelling_a_previewed_editor_restores_authored_bytes_with_a_fresh_plugin_token() {
    let (mut session, source, original, commands) = extrusion_editor();
    let bytes = original.doc.to_json();
    let historical = original.design_token();
    let rolled_back = session.design_token();
    session.preview(&commands).unwrap();
    session.restore_editor(&source, original).unwrap();
    assert_eq!(session.doc.to_json(), bytes);
    assert_ne!(session.design_token(), historical);
    assert_ne!(session.design_token(), rolled_back);
}

#[test]
fn editor_visibility_changes_allow_cancel_and_keep_undo_and_redo_at_the_original_marker() {
    for undo_hide in [false, true] {
        let (mut session, source, original, _) = extrusion_editor();
        let shown = session.visibility.clone();
        session.execute("browser.visibility", &json!({"items":["Other","XY"],"visible":false})).unwrap();
        let hidden = session.visibility.clone();
        if undo_hide {
            session.execute("edit.undo", &json!({})).unwrap();
        }
        let history = (session.undo.len(), session.redo.len());
        let live = session.visibility.clone();
        *session.model.cache.lock().unwrap() = Default::default();
        let evaluations = session.evaluations;
        session.restore_editor(&source, original).unwrap();
        assert_eq!(session.evaluations, evaluations, "Cancel reuses the pinned model");
        assert_eq!(session.visibility, live);
        assert_eq!((session.undo.len(), session.redo.len()), history);
        assert_editor_volume(&mut session, 6024.0);
        if undo_hide {
            session.execute("edit.redo", &json!({})).unwrap();
            assert_eq!(session.visibility, hidden);
        } else {
            session.execute("edit.undo", &json!({})).unwrap();
            assert_eq!(session.visibility, shown);
        }
        assert_editor_volume(&mut session, 6024.0);
    }
}

#[test]
fn visibility_changes_while_an_editor_computes_preserve_the_latest_view() {
    let (mut session, source, original, commands) = extrusion_editor();
    let prepared = session.scratch().compute_editor_commands(&commands, &source, original).unwrap();
    session.execute("browser.visibility", &json!({"items":["Other","XY"],"visible":false})).unwrap();
    let visibility = session.visibility.clone();
    assert!(prepared.is_current(&session));
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.visibility, visibility);
    assert_editor_volume(&mut session, 12024.0);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.visibility, visibility);
    assert_editor_volume(&mut session, 6024.0);
    session.execute("edit.undo", &json!({})).unwrap();
    assert_editor_volume(&mut session, 6024.0);
}

#[test]
fn visibility_does_not_make_a_stale_editor_current() {
    for change in 0..7 {
        let (mut session, source, original, commands) = extrusion_editor();
        match change {
            0 => session.revision += 1,
            1 => session.active_sketch = Some(2),
            2 => session.active_component = 17,
            3 => session.active_occurrence_path = Some(vec![1]),
            4 => {
                session.execute("solid.box", &json!({"length":1,"width":1,"height":1})).unwrap();
            }
            5 => {
                session.pending_moves.insert(1, solidcraft_doc::IDENTITY);
            }
            _ => session = Session::default(),
        }
        session.execute("browser.visibility", &json!({"items":["XY"],"visible":false})).unwrap();
        let doc = session.doc.clone();
        let visibility = session.visibility.clone();
        assert!(session.scratch().compute_editor_commands(&commands, &source, original.scratch()).is_err(), "change {change}");
        assert!(session.restore_editor(&source, original).is_err(), "change {change}");
        assert!(Arc::ptr_eq(&session.doc, &doc));
        assert_eq!(session.visibility, visibility);
    }
}

#[test]
fn accepting_commands_keeps_a_joint_shown_while_the_worker_ran() {
    let mut session = Session::default();
    let part = session.execute("component.create", &json!({"name":"Part","activate":false})).unwrap();
    let occurrence = part["occurrence"].as_u64().unwrap();
    let joint = session.execute("joint.as_built", &json!({"type":"revolute","a":0,"b":occurrence})).unwrap()["joint"].as_u64().unwrap();
    session.execute("browser.visibility", &json!({"joints":[joint],"visible":false})).unwrap();
    let prepared = session.scratch().compute_commands(&commands(), None).unwrap();
    session.execute("browser.visibility", &json!({"joints":[joint],"visible":true})).unwrap();
    session.execute("browser.visibility", &json!({"paths":[[occurrence]],"visible":false})).unwrap();
    let visibility = session.visibility.clone();
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.visibility, visibility, "the worker's old joint hide must not be merged back");
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.visibility, visibility, "undoing geometry keeps both concurrent visibility changes");
}

#[test]
fn cancelling_an_editor_with_visibility_changes_restores_an_existing_history_marker() {
    let (mut session, source, mut original, _) = extrusion_editor();
    original.execute("timeline.roll_to", &json!({"position":1})).unwrap();
    session.execute("browser.visibility", &json!({"items":["XY"],"visible":false})).unwrap();
    session.restore_editor(&source, original).unwrap();
    assert_eq!(session.doc.marker, Some(1));
    session.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc.marker, Some(1));
    assert!(session.visibility.hidden_origin.is_empty());
}

/// Rolling the marker back before a move of the body a joint seats on solves the joints again,
/// which puts the seated occurrence back where the body was: the editor's pinned model differs
/// from its source there, and only there, and the edit still goes through.
#[test]
fn an_editor_accepts_the_placements_rolling_the_marker_solves() {
    let mut session = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        session.execute("component.activate", &json!({"component": "root"})).unwrap();
        session.execute("component.create", &json!({"name": name})).unwrap();
        session.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    let component = |s: &Session, name: &str| s.doc.components.iter().find(|c| c.name == name).map(|c| c.id).unwrap();
    let occurrence = |s: &Session, name: &str| {
        let component = component(s, name);
        s.doc.occurrences.iter().find(|o| o.component == component).cloned().unwrap()
    };
    session.execute("occurrence.ground", &json!({"occurrence": occurrence(&session, "A").id, "grounded": true})).unwrap();
    session
        .execute("joint.create", &json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "offset": 5, "name": "Seat"}))
        .unwrap();
    let seated = occurrence(&session, "B").transform;
    // A's box moves along, and B, seated on it, with it.
    session.execute("component.activate", &json!({"component": "A"})).unwrap();
    let a = component(&session, "A");
    let body = session.model.state().bodies.iter().find(|b| b.component == a).map(|b| b.name.clone()).unwrap();
    session.execute("solid.move", &json!({"bodies": [body], "translate": [30, 0, 0]})).unwrap();
    let moved = occurrence(&session, "B").transform;
    assert_ne!(moved, seated, "B follows the box it is seated on");
    let original = session.scratch();
    session.execute("timeline.roll_to", &json!({"position": 2})).unwrap();
    let source = session.scratch();
    assert_eq!(occurrence(&session, "B").transform, seated, "before the move, B sits where the box was");
    let prepared = session.scratch().compute_editor_commands(&[], &source, original).unwrap();
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.doc.marker, None);
    assert_eq!(occurrence(&session, "B").transform, moved);
}

/// #18695: editing a feature in a design with joints whose values or expressions are re-solved
/// when the marker rolls back accepts the re-solved values: the editor's pinned model differs
/// from its source there, and OK still applies without "original editor model does not match".
#[test]
fn an_editor_accepts_the_joint_values_rolling_the_marker_solves() {
    let mut session = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        session.execute("component.activate", &json!({"component": "root"})).unwrap();
        session.execute("component.create", &json!({"name": name})).unwrap();
        session.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    let component = |s: &Session, name: &str| s.doc.components.iter().find(|c| c.name == name).map(|c| c.id).unwrap();
    let occurrence = |s: &Session, name: &str| {
        let component = component(s, name);
        s.doc.occurrences.iter().find(|o| o.component == component).cloned().unwrap()
    };
    session.execute("occurrence.ground", &json!({"occurrence": occurrence(&session, "A").id, "grounded": true})).unwrap();
    session
        .execute("joint.create", &json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "offset": "5 mm", "name": "Slide"}))
        .unwrap();
    session.execute("joint.drive", &json!({"joint": "Slide", "value": 15})).unwrap();
    assert_eq!(session.doc.assembly.joints[0].values, vec![15.0]);

    session.execute("component.activate", &json!({"component": "A"})).unwrap();
    let a = component(&session, "A");
    let body = session.model.state().bodies.iter().find(|b| b.component == a).map(|b| b.name.clone()).unwrap();
    session.execute("solid.move", &json!({"bodies": [body], "translate": [30, 0, 0]})).unwrap();

    let original = session.scratch();
    session.execute("timeline.roll_to", &json!({"position": 2})).unwrap();
    let source = session.scratch();

    let prepared = session.scratch().compute_editor_commands(&[], &source, original).unwrap();
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.doc.marker, None);
}

/// Cancel on a computing OK (solidcore#3247): a dialog's computation of a long Combine Cut (a
/// coil cut from a cylinder, as in #3962) stops within 500 ms of Cancel and says it was
/// cancelled. Nothing it computed is kept in the evaluation cache the session shares: OK again
/// computes the cut anew, and finishes.
#[test]
fn a_cancelled_computation_stops_promptly_and_is_never_cached() {
    use solidcraft_doc::progress::{Progress, watch};
    use std::time::{Duration, Instant};
    let mut session = Session::default();
    session.execute("solid.cylinder", &json!({"radius": 20, "height": 120, "body_name": "Stock"})).unwrap();
    let coil = json!({"diameter": 51, "pitch": 15.24, "turns": 6, "section": "square", "section_size": 13, "body_name": "Tool", "operation": "new", "base": [0, 0, 12]});
    session.execute("solid.coil", &coil).unwrap();
    let cut = vec![("solid.combine".to_string(), json!({"operation": "cut", "target": "Stock", "tools": ["Tool"]}))];
    let progress = Arc::new(Progress::default());
    let (watching, scratch, commands) = (progress.clone(), session.scratch(), cut.clone());
    let worker = std::thread::spawn(move || {
        let r = scratch.stop_token().within(|| watch(watching, || scratch.compute_commands(&commands, None)));
        (r.map(|_| ()).map_err(|e| e.to_string()), Instant::now())
    });
    std::thread::sleep(Duration::from_millis(300));
    let cancelled = Instant::now();
    progress.cancel();
    let (r, returned) = worker.join().unwrap();
    let took = returned.saturating_duration_since(cancelled);
    assert!(r.as_ref().is_err_and(|e| e.contains("cancelled")), "{r:?}");
    assert!(took < Duration::from_millis(500), "returned {took:?} after Cancel");
    let again = session.scratch().compute_commands(&cut, None).unwrap();
    // The cut is the one feature computed anew (the session's counter carries over to its copies).
    assert_eq!(again.computed.model.recomputed, session.model.recomputed + 1, "nothing of the cancelled run was cached");
    assert!(again.computed.model.results.iter().all(|r| r.error.is_none()), "OK again finishes the cut");
}

#[test]
fn computed_sketch_ownership_rejects_closed_targets_and_keeps_live_view_metadata() {
    use crate::{ViewContext, ViewDescriptor};
    use solidcraft_views::ViewKind;
    let context = |window| {
        ViewContext::new(
            vec![
                ViewDescriptor { id: 1, kind: ViewKind::Model, window: 0, subject: None },
                ViewDescriptor { id: 2, kind: ViewKind::Model, window, subject: None },
            ],
            1,
            vec![1, 2],
        )
        .unwrap()
    };
    let mut session = Session::default();
    session.set_view_context(context(1)).unwrap();
    let commands = vec![("sketch.create".into(), json!({"plane":"XY","view":2}))];
    let prepared = session.scratch().compute_commands(&commands, None).unwrap();
    session.set_view_context(ViewContext::default()).unwrap();
    let before = (session.doc.to_json(), session.undo.len(), session.revision);
    assert!(!prepared.is_current(&session));
    assert!(session.accept_commands(prepared).is_err());
    assert_eq!((session.doc.to_json(), session.undo.len(), session.revision), before);
    session.set_view_context(context(1)).unwrap();
    let mut computed = session.scratch();
    computed.execute("sketch.create", &json!({"view":2})).unwrap();
    session.set_view_context(ViewContext::default()).unwrap();
    assert!(session.accept_computed(computed).is_err());
    assert_eq!((session.doc.to_json(), session.undo.len(), session.revision), before);
    session.set_view_context(context(1)).unwrap();
    let prepared = session.scratch().compute_commands(&commands, None).unwrap();
    session.set_view_context(context(2)).unwrap();
    assert!(prepared.is_current(&session), "moving the same open Model is valid");
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.sketching_view, Some(2));
    assert_eq!(session.view_context, context(2), "adoption cannot reinstall the worker's stale window placement");
}
