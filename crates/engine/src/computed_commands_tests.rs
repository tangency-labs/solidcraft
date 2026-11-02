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
    prepared.warm_meshes(|| true);
    let world = prepared.computed.world_state();
    let before = session.evaluations;
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.evaluations, before);
    assert_eq!(session.path.as_deref(), Some("working.solidcraft"));
    assert!(Arc::ptr_eq(&saved, &session.saved));
    assert_eq!(session.undo.len(), history + 1);
    assert!(Arc::ptr_eq(&world, &session.world_state()));
    session.restore_checkpoint(checkpoint).unwrap();
    assert_eq!(session.undo.len(), history);
    assert_eq!(session.doc.features.len(), 1);
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
    let world = session.world_state();
    let original = session.scratch();
    session.execute("timeline.roll_to", &json!({"position":0})).unwrap();
    let source = session.scratch();
    *session.model.cache.lock().unwrap() = Default::default();
    let prepared = session.scratch().compute_editor_commands(&[], &source, original).unwrap();
    assert_eq!(prepared.computed.evaluations, 0, "restoring the marker must never evaluate the old suffix");
    assert!(Arc::ptr_eq(&world, &prepared.computed.world_state()));
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
