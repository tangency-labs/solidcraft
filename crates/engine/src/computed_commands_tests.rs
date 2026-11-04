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
fn checkpoint_restores_a_prepared_finish_without_putting_the_draft_in_document_undo() {
    let mut s = Session::default();
    let empty = s.checkpoint();
    s.execute("form.prototype.create", &json!({"primitive":"box","size":10,"segments":[1,1,1],"levels":1})).unwrap();
    s.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-6,-5,-5]]})).unwrap();
    let edited = s.execute("form.prototype.inspect", &json!({})).unwrap();
    s.execute("form.prototype.undo", &json!({})).unwrap();
    let draft = s.execute("form.prototype.inspect", &json!({})).unwrap();
    let open = s.checkpoint();
    let before = s.doc.clone();

    let prepared = s.scratch().compute_commands(&[("form.prototype.commit".into(), json!({"tolerance":0.05}))], None).unwrap();
    s.accept_commands(prepared).unwrap();
    assert!(s.forms_prototype.is_none());
    assert_eq!(s.doc.features.len(), 1);
    s.restore_checkpoint(open).unwrap();
    assert!(Arc::ptr_eq(&s.doc, &before));
    assert!(s.undo.is_empty());
    assert_eq!(s.execute("form.prototype.inspect", &json!({})).unwrap(), draft);
    s.execute("form.prototype.redo", &json!({})).unwrap();
    assert_eq!(s.execute("form.prototype.inspect", &json!({})).unwrap()["points"], edited["points"]);

    let open = s.checkpoint();
    s.execute("file.new", &json!({})).unwrap();
    assert!(s.forms_prototype.is_none());
    s.restore_checkpoint(open).unwrap();
    assert_eq!(s.execute("form.prototype.inspect", &json!({})).unwrap()["points"], edited["points"]);
    s.restore_checkpoint(empty).unwrap();
    assert!(s.forms_prototype.is_none());

    s.execute("form.prototype.create", &json!({"primitive":"box","size":10,"segments":[1,1,1],"levels":1})).unwrap();
    s.execute("form.prototype.commit", &json!({"tolerance":0.05})).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.forms_prototype.is_none());
    assert!(s.doc.features.is_empty());
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
fn view_invalidation_keeps_prepared_form_edits_current_across_tab_revision_jumps() {
    for offset in [0, 1_000] {
        let mut session = Session::default();
        session.execute("form.prototype.create", &json!({"size":10,"segments":[1,1,1],"levels":1})).unwrap();
        let command = ("form.prototype.set_vertices".into(), json!({"vertices":[0],"positions":[[-6,-5,5]]}));
        let prepared = session.scratch().compute_commands(&[command], None).unwrap();
        let document = session.doc.clone();
        let authoring = session.authoring_revision();
        session.refresh_display();
        let previous = session.revision + offset;
        session.invalidate_view(previous);
        assert!(session.revision > previous);
        assert_eq!(session.authoring_revision(), authoring);
        assert!(Arc::ptr_eq(&document, &session.doc));
        assert!(prepared.is_current(&session));
        session.accept_commands(prepared).unwrap();
        assert_eq!(session.authoring_revision(), authoring, "adopting a transient Form edit must not invent a document/model change");
        assert_eq!(session.execute("form.prototype.inspect", &json!({})).unwrap()["undo"], 1);
        assert_eq!(session.forms_prototype.as_ref().unwrap().cage.points[0], solidcore_record::Vec3::new(-6., -5., 5.));
    }
}

#[test]
fn acceptance_keeps_the_computed_authoring_revision_after_a_large_display_jump() {
    let mut session = Session::default();
    session.refresh_display();
    let original = session.authoring_revision();
    let prepared = session.scratch().compute_commands(&commands(), None).unwrap();
    let expected = prepared.computed.authoring_revision();
    assert!(expected > original, "the prepared box really changed the document/model");
    session.invalidate_view(session.revision + 100_000);
    assert_eq!(session.authoring_revision(), original);
    assert!(prepared.is_current(&session));
    session.accept_commands(prepared).unwrap();
    assert_eq!(session.authoring_revision(), expected, "revision and display-only increments must be adopted as one history");
    let previous = session.revision;
    session.invalidate_view(previous + 100_000);
    assert_eq!(session.authoring_revision(), expected);
    assert_eq!(session.doc.features.len(), 1);
}

#[test]
fn view_invalidation_does_not_hide_authored_source_context_or_support_changes() {
    for change in 0..9 {
        let mut session = Session::default();
        session.execute("solid.box", &json!({"length":2,"width":3,"height":4,"body_name":"Support"})).unwrap();
        let primitive = json!({"size":10,"segments":[1,1,1],"levels":1});
        session.execute("form.prototype.create", &primitive).unwrap();
        let command = ("form.prototype.set_vertices".into(), json!({"vertices":[0],"positions":[[-6,-5,5]]}));
        let prepared = session.scratch().compute_commands(&[command], None).unwrap();
        let authoring = session.authoring_revision();
        session.invalidate_view(session.revision + 1_000);
        assert_eq!(session.authoring_revision(), authoring);
        assert!(prepared.is_current(&session));
        match change {
            0 => {
                session.execute("parameters.add", &json!({"name":"changed","expression":"2 mm"})).unwrap();
            }
            1 => {
                session.execute("form.prototype.set_vertices", &json!({"vertices":[0],"positions":[[-7,-5,5]]})).unwrap();
            }
            2 => {
                session.execute("form.prototype.close", &json!({})).unwrap();
                session.execute("form.prototype.create", &primitive).unwrap();
            }
            3 => session.active_component = 17,
            4 => session.active_occurrence_path = Some(vec![1]),
            5 => session.selection.push(crate::Sel::Axis { name: "X".into() }),
            6 => {
                session
                    .pending_moves
                    .insert(41, crate::doc::rigid(solidcore_record::Vec3::X, solidcore_record::Vec3::ZERO, solidcore_record::Vec3::Z, 0.0));
            }
            7 => {
                let document = session.doc.clone();
                session.refresh();
                assert!(Arc::ptr_eq(&document, &session.doc));
                assert!(session.authoring_revision() > authoring, "a model refresh is not display-only");
            }
            _ => {
                let crate::doc::FeatureKind::Box { height, .. } = &mut session.doc_mut().features[0].kind else { panic!("expected support box") };
                *height = "8 mm".into();
                session.refresh();
            }
        }
        let document = session.doc.clone();
        let draft = session.forms_prototype.as_ref().unwrap().definition();
        assert!(!prepared.is_current(&session), "source change {change} must refuse old work");
        assert!(session.accept_commands(prepared).is_err());
        assert!(Arc::ptr_eq(&document, &session.doc));
        assert_eq!(session.forms_prototype.as_ref().unwrap().definition(), draft);
    }
}

#[test]
fn prepared_form_commands_reject_an_identical_replacement_draft() {
    let mut session = Session::default();
    let primitive = json!({"size":10,"segments":[1,1,1],"levels":1});
    session.execute("form.prototype.create", &primitive).unwrap();
    let edit = ("form.prototype.set_vertices".into(), json!({"vertices":[0],"positions":[[-6,-5,-5]]}));
    let prepared = session.scratch().compute_commands(&[edit], None).unwrap();
    assert!(prepared.is_current(&session));
    let original = session.execute("form.prototype.inspect", &json!({})).unwrap();

    session.execute("form.prototype.close", &json!({})).unwrap();
    session.execute("form.prototype.create", &primitive).unwrap();
    assert_eq!(session.execute("form.prototype.inspect", &json!({})).unwrap(), original);
    assert!(session.accept_commands(prepared).is_err());
    assert_eq!(session.execute("form.prototype.inspect", &json!({})).unwrap(), original);
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
fn editor_tab_view_invalidations_allow_apply_and_cancel() {
    for jump in [0, 10_000] {
        let (mut session, source, original, commands) = extrusion_editor();
        let prepared = session.scratch().compute_editor_commands(&commands, &source, original).unwrap();
        let authoring = session.authoring_revision();
        session.invalidate_view(session.revision + jump);
        assert_eq!(session.authoring_revision(), authoring);
        assert!(prepared.is_current(&session));
        session.accept_commands(prepared).unwrap();
        assert_editor_volume(&mut session, 12024.0);

        let (mut session, source, original, _) = extrusion_editor();
        let authoring = source.authoring_revision();
        let history = (session.undo.len(), session.redo.len());
        session.invalidate_view(session.revision + jump);
        session.restore_editor(&source, original).unwrap();
        assert_eq!(source.authoring_revision(), authoring, "the saved editor source remains untouched");
        assert_eq!((session.undo.len(), session.redo.len()), history);
        assert!(session.doc.marker.is_none());
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
        session.invalidate_view(session.revision + 10_000);
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
