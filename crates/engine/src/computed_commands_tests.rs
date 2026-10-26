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
