//! Edit Feature must replace the selections as well as the numerical definition.

use serde_json::{Value, json};

use crate::{Sel, Session};

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    s.execute(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"))
}

#[test]
fn select_geometry_resolves_points_and_indices_without_changing_history() {
    use solidcraft_geom::Vec3;

    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"body_name":"Block", "corner":[20,30,40], "length":10, "width":10, "height":10}));
    let before = (s.doc.to_json(), s.undo.len());
    for (kind, point) in [("face", Vec3::new(25.0, 35.0, 50.0)), ("edge", Vec3::new(25.0, 30.0, 50.0))] {
        assert_eq!(run(&mut s, "select.set", json!({"items":[{"type":kind,"body":"Block","point":point}]}))["selected"], 1);
        let index = match &s.selection[0] {
            Sel::Face { index, point: actual, .. } | Sel::Edge { index, point: actual, .. } => {
                assert_eq!(*actual, point);
                *index
            }
            other => panic!("unexpected selection: {other:?}"),
        };
        let measured = s.measure_items(&s.selection);
        let (field, expected) = if kind == "face" { ("area_mm2", 100.0) } else { ("length_mm", 10.0) };
        assert!((measured["items"][0][field].as_f64().unwrap() - expected).abs() < 1e-9, "{measured}");
        run(&mut s, "select.set", json!({"items":[{"type":kind,"body":"Block","index":index}]}));
        let complete = serde_json::to_value(&s.selection[0]).unwrap();
        assert_eq!(complete["index"], index);
        let mut point_only = complete.clone();
        point_only.as_object_mut().unwrap().remove("index");
        run(&mut s, "select.set", json!({"items":[point_only]}));
        assert_eq!(serde_json::to_value(&s.selection[0]).unwrap(), complete);
    }
    assert_eq!((s.doc.to_json(), s.undo.len()), before);
}

#[test]
fn select_geometry_curved_picks_round_trip_and_add_without_duplicates() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"body_name":"Round", "radius":5, "height":10}));
    for (kind, point) in [("face", [3, 4, 5]), ("edge", [3, 4, 10])] {
        run(&mut s, "select.set", json!({"items":[{"type":kind,"body":"Round","point":point}]}));
        let index = serde_json::to_value(&s.selection[0]).unwrap()["index"].clone();
        run(&mut s, "select.set", json!({"items":[{"type":kind,"body":"Round","index":index}]}));
        let complete = serde_json::to_value(&s.selection[0]).unwrap();
        let pick: solidcraft_geom::Vec3 = serde_json::from_value(complete["point"].clone()).unwrap();
        assert!((pick.x.hypot(pick.y) - 5.0).abs() < 1e-6, "the inferred point lies on the cylinder: {complete}");
        let mut partial = complete.clone();
        partial.as_object_mut().unwrap().remove("index");
        run(&mut s, "select.set", json!({"items":[partial, complete],"add":true}));
        assert_eq!(s.selection.len(), 1);
        assert_eq!(serde_json::to_value(&s.selection[0]).unwrap(), complete);
        let measured = s.measure_items(&s.selection);
        assert!((measured["items"][0]["diameter"].as_f64().unwrap() - 10.0).abs() < 1e-6, "{measured}");
    }
}

#[test]
fn select_geometry_uses_the_placed_body_in_world_coordinates() {
    let mut s = Session::default();
    let created = run(&mut s, "component.create", json!({"name":"Part"}));
    run(&mut s, "solid.box", json!({"body_name":"Block", "length":2, "width":3, "height":4}));
    let second = run(&mut s, "occurrence.copy", json!({"occurrence":created["occurrence"]}))["occurrence"].as_u64().unwrap();
    run(&mut s, "occurrence.move", json!({"occurrence":second,"translate":[10,20,0],"axis":[0,0,1],"angle":"90 deg","capture":false}));
    let name = s.occurrence_placement(&[second]).unwrap().bodies[0].world.clone();
    for (kind, point) in [("face", [8.5, 21.0, 4.0]), ("edge", [8.5, 20.0, 4.0])] {
        run(&mut s, "select.set", json!({"items":[{"type":kind,"body":name,"point":point}]}));
        let complete = serde_json::to_value(&s.selection[0]).unwrap();
        assert_eq!(complete["body"], name);
        run(&mut s, "select.set", json!({"items":[{"type":kind,"body":name,"index":complete["index"]}]}));
        let inferred = serde_json::to_value(&s.selection[0]).unwrap();
        let pick: solidcraft_geom::Vec3 = serde_json::from_value(inferred["point"].clone()).unwrap();
        assert!((pick.z - 4.0).abs() < 1e-9 && (7.0..=10.0).contains(&pick.x) && (20.0..=22.0).contains(&pick.y), "{inferred}");
        assert!(s.execute("select.set", &json!({"items":[{"type":kind,"body":name,"point":[1,1,4]}]})).is_err());
        assert_eq!(serde_json::to_value(&s.selection[0]).unwrap(), inferred);
    }
}

#[test]
fn select_geometry_invalid_partial_picks_leave_selection_and_revision_unchanged() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"body_name":"Block", "length":10, "width":10, "height":10}));
    run(&mut s, "select.set", json!({"items":[{"type":"body","name":"Block"}]}));
    let before = (s.selection.clone(), s.revision, s.undo.len());
    for kind in ["face", "edge"] {
        for fields in [
            json!({}),
            json!({"index":999999}),
            json!({"index":-1}),
            json!({"index":"0"}),
            json!({"point":[500,500,500]}),
            json!({"point":[1,2]}),
            json!({"point":[1e308,1e308,1e308]}),
        ] {
            let mut item = json!({"type":kind,"body":"Block"});
            item.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
            for add in [false, true] {
                let result = s.execute("select.set", &json!({"items":[{"type":"face","body":"Block","point":[5,5,10]},item],"add":add}));
                assert!(result.is_err(), "accepted {item}");
                assert_eq!((s.selection.clone(), s.revision, s.undo.len()), before, "{item}");
            }
        }
    }
    assert!(s.execute("select.set", &json!({"items":[{"type":"face","body":"Missing","point":[5,5,10]}]})).is_err());
    assert_eq!((s.selection, s.revision, s.undo.len()), before);
}

#[test]
fn history_playback_is_a_typed_display_request_without_history_edits() {
    let mut s = Session::default();
    let revision = s.revision;
    let undo = s.undo.len();
    for action in ["toggle", "play", "stop"] {
        assert_eq!(run(&mut s, "timeline.play", json!({"action": action})), json!({"action": action}));
    }
    assert_eq!(run(&mut s, "timeline.play", json!({})), json!({"action": "toggle"}));
    for action in [json!("pause"), json!(true), json!(1), json!([])] {
        assert!(s.execute("timeline.play", &json!({"action": action})).is_err());
    }
    assert_eq!(s.revision, revision);
    assert_eq!(s.undo.len(), undo);
    assert!(s.doc.marker.is_none());
    let description = run(&mut s, "engine.describe", json!({"command": "timeline.play"}));
    assert!(description["input"].is_object());
    assert!(description["output"].is_object());
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    assert!(s.execute("timeline.play", &json!({})).is_err());
}

/// `timeline.roll_to` takes a number of features or "end"; any other value is refused instead of
/// silently moving the marker to the end.
#[test]
fn roll_to_takes_a_number_or_end_and_refuses_anything_else() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 20, "operation": "join"}));
    for cmd in ["timeline.roll_to", "timeline.rollback"] {
        assert_eq!(run(&mut s, cmd, json!({"position": 1}))["marker"], 1);
        assert_eq!(run(&mut s, cmd, json!({"position": "end"}))["marker"], 2);
        assert_eq!(run(&mut s, cmd, json!({"position": "1"}))["marker"], 1, "a number in a string is still a number");
        assert_eq!(run(&mut s, cmd, json!({"position": "END"}))["marker"], 2);
        run(&mut s, cmd, json!({"position": 1}));
        for bad in [json!("start"), json!("first"), json!([1]), json!({"n": 1}), json!(true), json!("NaN")] {
            let e = s.execute(cmd, &json!({"position": bad})).unwrap_err().to_string();
            assert!(e.contains("`position` must be a number") && e.contains("\"end\""), "{bad}: {e}");
            assert_eq!(s.doc.marker, Some(1), "{bad}: the refused call left the marker where it was");
        }
        assert_eq!(run(&mut s, cmd, json!({}))["marker"], 2, "omitted: the end");
        // #4712: a negative position is refused (it rolled the whole timeline back), in the
        // name of the command asked for.
        run(&mut s, cmd, json!({"position": 1}));
        for neg in [json!(-1), json!("-1"), json!(-0.5)] {
            let e = s.execute(cmd, &json!({"position": neg})).unwrap_err().to_string();
            assert!(e.contains("negative") && e.contains(&format!("`{cmd}`")), "{neg}: {e}");
            assert_eq!(s.doc.marker, Some(1), "{neg}: the marker stays");
        }
        let e = s.execute(cmd, &json!({"position": "start"})).unwrap_err().to_string();
        assert!(e.contains(&format!("`{cmd}`")), "named as asked: {e}");
    }
}

/// #7077: Repeat Last Command is a command (its key can change, command search finds it), but
/// only the app has a tool to start, so a script's call is refused and changes nothing.
#[test]
fn repeat_is_refused_outside_the_app_and_changes_nothing() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let (doc, undo) = (s.doc.to_json(), s.undo.len());
    let e = s.execute("edit.repeat", &json!({})).unwrap_err().to_string();
    assert!(e.contains("in the app"), "{e}");
    assert_eq!((s.doc.to_json(), s.undo.len()), (doc, undo));
    let spec = crate::find_command("edit.repeat").unwrap();
    assert_eq!((spec.shortcut, spec.tab, spec.undoable), (Some("Shift+R"), "", false));
}

#[test]
fn creating_after_deleting_future_features_keeps_the_new_feature_active() {
    let mut s = Session::default();
    for (name, corner, size) in [("Base", [0, 0, 0], 10), ("Later1", [20, 0, 0], 2), ("Later2", [40, 0, 0], 3)] {
        run(&mut s, "solid.box", json!({"name": name, "corner": corner, "length": size, "width": size, "height": size, "operation": "new"}));
    }

    run(&mut s, "timeline.roll_to", json!({"position": 1}));
    run(&mut s, "solid.box", json!({"name": "Inserted", "corner": [60, 0, 0], "length": 4, "width": 4, "height": 4, "operation": "new"}));
    assert_eq!(s.doc.features.iter().map(|feature| feature.name.as_str()).collect::<Vec<_>>(), ["Base", "Inserted", "Later1", "Later2"]);
    assert_eq!(s.doc.marker, Some(2), "with later features, the new feature is inserted at the cursor");

    run(&mut s, "timeline.delete", json!({"features": ["Later1", "Later2"]}));
    assert_eq!(s.doc.features.len(), 2);
    assert_eq!(s.doc.marker, Some(2), "the cursor is at the end after deleting all later features");

    run(&mut s, "solid.box", json!({"name": "Appended", "corner": [80, 0, 0], "length": 5, "width": 5, "height": 5, "operation": "new"}));

    assert_eq!(s.doc.features.iter().map(|feature| feature.name.as_str()).collect::<Vec<_>>(), ["Base", "Inserted", "Appended"]);
    assert_eq!(s.doc.marker, Some(3), "the new feature advances the cursor to the end");
    assert_eq!(s.model.state().bodies.len(), 3, "the appended feature is evaluated, not left in the future");
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - 1189.0).abs() < 1e-6, "{volume}");
}

#[test]
fn g2_fillet_survives_edit_undo_and_reopen() {
    use solidcraft_doc::{Document, FeatureKind, FilletStyle};
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "name": "Base"}));
    run(&mut s, "solid.fillet", json!({"edges": [[20, 0, 10]], "radius": 2, "type": "g2", "name": "Smooth"}));
    assert!(matches!(&s.doc.find_feature("Smooth").unwrap().kind, FeatureKind::Fillet { style: FilletStyle::Curvature, .. }));
    let before = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    run(&mut s, "timeline.edit", json!({"feature": "Smooth", "set": {"radius": "3"}}));
    let after = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!(after < before);
    run(&mut s, "edit.undo", json!({}));
    let undo = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((undo - before).abs() < 1e-6);
    run(&mut s, "edit.redo", json!({}));
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    assert!(matches!(&reopened.doc.find_feature("Smooth").unwrap().kind, FeatureKind::Fillet { style: FilletStyle::Curvature, .. }));
    let volume = run(&mut reopened, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - after).abs() < 1e-6);
    assert!(s.execute("solid.fillet", &json!({"edges": [[0, 0, 10]], "radius": 2, "type": "curvature", "fit": "reduce"})).is_err());
}

#[test]
fn redefining_a_fillet_as_g2_on_sloped_ends_survives_undo_and_reopen() {
    use solidcraft_doc::{Document, FeatureKind, FilletStyle};
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [30, 20], [10, 20]], "closed": true}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 20}));
    run(&mut s, "solid.fillet", json!({"edges": [[20, 0, 20]], "radius": 5, "name": "Fillet1"}));
    let original = s.doc.clone();
    let id = s.doc.find_feature("Fillet1").unwrap().id;
    // #2708: changing an existing constant fillet to Curvature (G2) in Edit Feature.
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": id, "command": "solid.fillet", "params": {"edges": [[20, 0, 20]], "radius": "5", "type": "curvature"}}),
    );
    let f = s.doc.find_feature("Fillet1").unwrap();
    assert_eq!(f.id, id);
    assert!(matches!(&f.kind, FeatureKind::Fillet { style: FilletStyle::Curvature, .. }));
    // Use a fixed fine chord tolerance for the analytic comparison; Measure's
    // adaptive display tolerance is coarser and need not satisfy this bound.
    let volume = |s: &Session| {
        let bodies = &s.model.state().bodies;
        assert_eq!(bodies.len(), 1);
        bodies.first().unwrap().body.tessellate(0.0001).unwrap().measure().volume
    };
    let expected = |r: f64| 12000.0 - 40.0 * 113.0 / 756.0 * r.powi(2) + 13711.0 / 486486.0 * r.powi(3);
    let actual = volume(&s);
    assert!((actual - expected(5.0)).abs() < 0.05, "{actual} vs {}", expected(5.0));
    let edited = s.doc.clone();
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc, original);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc, edited);
    let mut reopened = Session::new(Document::from_json(&s.doc.to_json()).unwrap());
    assert!((volume(&reopened) - expected(5.0)).abs() < 0.05);
    run(&mut reopened, "timeline.edit", json!({"feature": id, "set": {"radius": "3"}}));
    assert!((volume(&reopened) - expected(3.0)).abs() < 0.05);
    let before = reopened.doc.clone();
    assert!(
        reopened
            .execute(
                "timeline.redefine",
                &json!({"feature": id, "command": "solid.fillet", "params": {"edges": [[20, 0, 20]], "radius": 100, "type": "curvature"}})
            )
            .is_err()
    );
    assert_eq!(reopened.doc, before);
    assert!((volume(&reopened) - expected(3.0)).abs() < 0.05);
}

#[test]
fn redefining_a_blend_replaces_persistent_edge_selection_and_survives_resize() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "name": "Base"}));
    run(&mut s, "solid.fillet", json!({"edges": [[20, 0, 20]], "radius": 2, "name": "Round"}));
    let round_id = s.doc.find_feature("Round").unwrap().id;
    let body_name = s.model.state().bodies[0].name.clone();
    s.doc_mut().feature_mut(round_id).unwrap().participants = vec![body_name];
    s.refresh();
    let old = s.doc.find_feature("Round").unwrap().clone();
    let before = (*s.doc).clone();
    run(&mut s, "timeline.redefine", json!({"feature": "Round", "command": "solid.fillet", "params": {"edges": [[0, 15, 20]], "radius": 2}}));
    let edited = s.doc.find_feature("Round").unwrap().clone();
    assert_eq!(edited.id, old.id);
    assert_eq!(edited.param_names, old.param_names);
    assert_eq!(edited.participants, old.participants);
    assert_ne!(edited.edge_names, old.edge_names, "the new edge must be the reference used on recompute");
    assert_ne!(edited.edge_refs, old.edge_refs);
    assert_eq!(s.doc.next_id, before.next_id, "temporary definitions must not consume feature ids");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*s.doc, before);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.find_feature("Round").unwrap().edge_names, edited.edge_names);
    run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"length": "60", "height": "25"}}));
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    let expected = 60.0 * 30.0 * 25.0 - (4.0 - std::f64::consts::PI) * 30.0;
    assert!((volume - expected).abs() < expected * 1e-4, "{volume} != {expected}");
    assert!(s.model.result(old.id).unwrap().warning.is_none());
}

/// A redefine computes the features after the edited one once: its trial's results are kept
/// for the session's own evaluation, though the cache holds only the last state (a large
/// design's). The reply reports that one rebuild (#5694): a session that had to compute the
/// features again would count them twice.
#[test]
fn redefining_computes_the_later_features_once() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 5, "name": "Base"}));
    for x in [30, 40, 50] {
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [x, 0, 0]}));
    }
    *s.model.cache.lock().unwrap() = solidcraft_doc::EvalCache::with_budget(1);
    let reply =
        run(&mut s, "timeline.redefine", json!({"feature": "Base", "command": "solid.box", "params": {"length": 25, "width": 10, "height": 5}}));
    assert_eq!((reply["recomputed"].as_u64(), reply["restored"].as_u64()), (Some(4), Some(0)), "{reply}");
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - (25.0 * 10.0 * 5.0 + 3.0 * 125.0)).abs() < 1e-6, "{volume}");
}

/// #5694: the reply reports what the redefine rebuilt — the edited feature and the features
/// after it — not the session's reuse of the trial's own results, which read as "nothing was
/// rebuilt" and did not match what the same change reports through `timeline.edit`.
#[test]
fn redefining_reports_the_features_it_rebuilt() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 20, "center": [0, 0, 10], "name": "Block"}));
    run(&mut s, "solid.hole", json!({"position": [0, 0, 20], "diameter": 6, "depth": 12, "name": "H"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 5, "center": [30, 0, 2.5], "operation": "new", "name": "Other"}));
    let reply = run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "H", "command": "solid.hole",
               "params": {"position": [0, 0, 20], "type": "counterbore", "diameter": 6, "depth": 12, "cb_diameter": 10, "cb_depth": 4}}),
    );
    assert_eq!((reply["recomputed"].as_u64(), reply["restored"].as_u64()), (Some(2), Some(0)), "{reply}");
}

#[test]
fn redefining_preserves_body_names_participants_and_marker_without_temporary_features() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 5, "name": "Base", "body_name": "Housing"}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [30, 0, 0], "name": "Other"}));
    let id = s.doc.find_feature("Base").unwrap().id;
    run(&mut s, "timeline.rollback", json!({"position": 1}));
    let before = s.doc.feature(id).unwrap().clone();
    run(&mut s, "timeline.redefine", json!({"feature": id, "command": "solid.box", "params": {"length": 25, "width": 10, "height": 5}}));
    let after = s.doc.feature(id).unwrap();
    assert_eq!(s.doc.features.len(), 2);
    assert_eq!(s.doc.marker, Some(1));
    assert_eq!(after.body_names, before.body_names);
    assert_eq!(after.participants, before.participants);
    assert_eq!(after.param_names, before.param_names);
    assert_eq!(after.name, before.name);
    assert!(s.model.state().body("Housing").is_some());
}

#[test]
fn failed_or_non_feature_redefinitions_leave_session_and_history_intact() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 10, "height": 5, "name": "Base"}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s.selection = vec![Sel::Feature { id: s.doc.find_feature("Base").unwrap().id }];
    let doc = s.doc.clone();
    let sketch = s.active_sketch;
    let selection = s.selection.clone();
    let undo = s.undo.len();
    for (command, params) in [
        ("solid.cylinder", json!({"radius": 5, "height": 10})),
        ("solid.box", json!({"length": "unknown_parameter", "width": 10, "height": 5})),
        ("file.export", json!({"path": "should-not-be-created.step"})),
        ("edit.undo", json!({})),
    ] {
        assert!(s.execute("timeline.redefine", &json!({"feature": "Base", "command": command, "params": params})).is_err());
        assert_eq!(*s.doc, *doc, "{command}");
        assert_eq!(s.active_sketch, sketch, "{command}");
        assert_eq!(s.selection, selection, "{command}");
        assert_eq!(s.undo.len(), undo, "{command}");
    }
}

#[test]
fn redefining_an_extrude_names_new_optional_inputs() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5, "name": "Pad"}));
    let distance = s.doc.find_feature("Pad").unwrap().param_names[0].clone();
    run(&mut s, "timeline.redefine", json!({"feature": "Pad", "command": "solid.extrude", "params": {"distance": 5, "start_offset": 2}}));
    let f = s.doc.find_feature("Pad").unwrap();
    assert_eq!(f.param_names.len(), f.kind.inputs().len());
    assert_eq!(f.param_names[0], distance);
    assert!(f.param_names.iter().all(|name| !name.is_empty()));
    assert_eq!(s.doc.features.len(), 2);
    let offset = s.doc.find_feature("Pad").unwrap().param_names.last().unwrap().clone();
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Pad", "command": "solid.extrude", "params": {"distance": 5, "distance2": 2, "start_offset": 2}}),
    );
    let f = s.doc.find_feature("Pad").unwrap();
    let inputs = f.kind.inputs();
    let offset_index = inputs.iter().position(|(label, _, _)| *label == "Start offset").unwrap();
    assert_eq!(f.param_names[offset_index], offset, "adding Distance 2 must not steal the offset dimension's parameter name");
    let distance2_index = inputs.iter().position(|(label, _, _)| *label == "Distance 2").unwrap();
    assert_ne!(f.param_names[distance2_index], offset);
}

#[test]
fn redefining_a_shell_replaces_persistent_face_selection() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "name": "Base"}));
    run(&mut s, "solid.shell", json!({"faces": [[20, 15, 20]], "thickness": 2, "name": "Hollow"}));
    let before = s.doc.find_feature("Hollow").unwrap().face_names.clone();
    assert!(!before.is_empty());
    run(&mut s, "timeline.redefine", json!({"feature": "Hollow", "command": "solid.shell", "params": {"faces": [[20, 15, 0]], "thickness": 2}}));
    let after = s.doc.find_feature("Hollow").unwrap().face_names.clone();
    assert!(!after.is_empty());
    assert_ne!(before, after);
    run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"height": "30"}}));
    let id = s.doc.find_feature("Hollow").unwrap().id;
    assert!(s.model.result(id).unwrap().error.is_none());
    assert!(s.model.result(id).unwrap().warning.is_none());
    assert_eq!(s.doc.find_feature("Hollow").unwrap().face_names, after);
}

#[test]
fn redefine_rejects_sheet_exports_before_they_touch_any_files() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [30, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"name": "Plate"}));
    let doc = s.doc.clone();
    let base = std::env::temp_dir().join(format!("solidcraft-redefine-export-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    for command in ["sheet.export_dxf", "sheet.export_bend_table"] {
        for existing in [false, true] {
            let path = base.join(format!("{}-{existing}.out", command.replace('.', "-")));
            if existing {
                std::fs::write(&path, b"original file").unwrap();
            } else if path.exists() {
                std::fs::remove_file(&path).unwrap();
            }
            let error = s.execute("timeline.redefine", &json!({"feature": "Plate", "command": command, "params": {"path": path}})).unwrap_err();
            assert!(error.to_string().contains("does not make a feature"), "{error}");
            assert_eq!(*s.doc, *doc);
            if existing {
                assert_eq!(std::fs::read(&path).unwrap(), b"original file");
                std::fs::remove_file(&path).unwrap();
            } else {
                assert!(!path.exists(), "{command} must not create a file before refusing the edit");
            }
        }
    }
    for command in ["sheet.manage_rules", "sheet.rules", "sheet.flat_pattern", "parameters.list", "plastic.manage_rules"] {
        let error = s.execute("timeline.redefine", &json!({"feature": "Plate", "command": command, "params": {"name": "unexpected"}})).unwrap_err();
        assert!(error.to_string().contains("does not make a feature"), "{error}");
        assert_eq!(*s.doc, *doc);
    }
    std::fs::remove_dir(&base).unwrap();
}

#[test]
fn redefine_keeps_allocator_above_component_and_occurrence_ids() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "Base"}));
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [10, 0, 0], "body_name": "Tool"}));
    run(&mut s, "solid.combine", json!({"target": "Base", "tools": ["Tool"], "name": "Join"}));
    let original = s.doc.clone();
    let original_feature = s.doc.find_feature("Join").unwrap().id;
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Join", "command": "solid.combine", "params": {"target": "Base", "tools": ["Tool"], "new_component": true, "component_name": "Merged"}}),
    );
    let destination = s.doc.find_component("Merged").unwrap();
    assert_eq!(s.model.state().body("Base").unwrap().component, destination);
    assert_eq!(s.doc.find_feature("Join").unwrap().id, original_feature);
    let combine_index = s.doc.feature_index(original_feature).unwrap();
    assert!(
        matches!(&s.doc.features[combine_index + 1].kind, solidcraft_doc::FeatureKind::BodyTransfer { bodies, to, .. } if bodies == &["Base"] && *to == destination)
    );
    assert!((solidcraft_kernel::measure(&s.world_state().body("Base").unwrap().body).unwrap().volume - 6000.0).abs() < 1e-6);
    assert_eq!(s.world_state().bodies.len(), 1);
    run(&mut s, "timeline.rollback", json!({"position":combine_index + 1}));
    assert_eq!(s.world_state().body("Base").unwrap().component, 0, "the result transfers only AFTER Combine");
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*s.doc, *original);
    run(&mut s, "edit.redo", json!({}));
    let reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
    assert_eq!(reopened.world_state().body("Base").unwrap().component, destination);
    assert!((solidcraft_kernel::measure(&reopened.world_state().body("Base").unwrap().body).unwrap().volume - 6000.0).abs() < 1e-6);
    let max_id =
        s.doc.components.iter().map(|c| c.id).chain(s.doc.occurrences.iter().map(|o| o.id)).chain(s.doc.features.iter().map(|f| f.id)).max().unwrap();
    assert!(s.doc.next_id > max_id);
    run(&mut s, "component.create", json!({"name": "Next", "parent": 0}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2}));
    let ids: Vec<u64> =
        s.doc.components.iter().map(|c| c.id).chain(s.doc.occurrences.iter().map(|o| o.id)).chain(s.doc.features.iter().map(|f| f.id)).collect();
    let unique: std::collections::BTreeSet<u64> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "feature, component and occurrence ids must remain unique: {ids:?}");
}

#[test]
fn redefine_cannot_publish_a_second_transfer_that_breaks_existing_ownership() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"body_name":"Base"}));
    run(&mut s, "solid.box", json!({"length":2,"width":2,"height":2,"corner":[1,0,0],"body_name":"Tool"}));
    run(&mut s, "solid.combine", json!({"target":"Base","tools":["Tool"],"name":"Join","new_component":true,"component_name":"Original"}));
    let full = s.doc.to_json();
    let join_index = s.doc.features.iter().position(|feature| feature.name == "Join").unwrap();
    for marker in [None, Some(join_index), Some(join_index + 1)] {
        let mut s = Session::new(solidcraft_doc::Document::from_json(&full).unwrap());
        s.doc_mut().marker = marker;
        s.refresh();
        let before = s.doc.to_json();
        let pending = s.pending_moves.clone();
        let error = s.execute("timeline.redefine", &json!({"feature":"Join","command":"solid.combine","params":{"target":"Base","tools":["Tool"],"new_component":true,"component_name":"Duplicate"}})).unwrap_err();
        assert!(error.to_string().contains("transfer source component"), "marker {marker:?}: {error}");
        assert_eq!(s.doc.to_json(), before);
        assert_eq!(s.pending_moves, pending);
    }
    let reopened = Session::new(solidcraft_doc::Document::from_json(&full).unwrap());
    assert_eq!(reopened.world_state().body("Base").unwrap().component, reopened.doc.find_component("Original").unwrap());
    assert!(reopened.doc.features.iter().all(|feature| reopened.model.result(feature.id).is_some_and(|result| result.error.is_none())));
}

#[test]
fn redefine_maps_placement_inputs_in_the_feature_owner_frame() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "name": "RootBox", "body_name": "RootBody"}));
    let owner = run(&mut s, "component.create", json!({"name": "Owner", "grounded": false}));
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [10, 0, 0], "name": "OwnedBox", "body_name": "OwnedBody"}));
    run(&mut s, "solid.fillet", json!({"edges": [[12, 0, 4]], "radius": 0.5, "body": "OwnedBody", "name": "OwnerRound"}));
    run(&mut s, "occurrence.move", json!({"occurrence": owner["occurrence"], "translate": [100, 0, 0]}));
    let other = run(&mut s, "component.create", json!({"name": "Other", "parent": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": other["occurrence"], "translate": [200, 0, 0]}));
    let active = s.active_component;
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "RootBox", "command": "solid.box", "params": {"length": 4, "width": 4, "height": 4, "corner": [2, 0, 0]}}),
    );
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "OwnedBox", "command": "solid.box", "params": {"length": 4, "width": 4, "height": 4, "corner": [110, 0, 0]}}),
    );
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "OwnerRound", "command": "solid.fillet", "params": {"edges": [[112, 0, 4]], "radius": 0.75, "body": "OwnedBody"}}),
    );
    assert_eq!(s.active_component, active, "editing must not activate the owner in the real session");
    assert_eq!(s.doc.find_feature("OwnedBox").unwrap().component, owner["component"].as_u64().unwrap());
    let root = run(&mut s, "inspect.measure", json!({"bodies": ["RootBody"]}));
    let owned = run(&mut s, "inspect.measure", json!({"bodies": ["OwnedBody"]}));
    assert!((root["bodies"][0]["bbox"]["min"][0].as_f64().unwrap() - 2.0).abs() < 1e-8);
    assert!((owned["bodies"][0]["bbox"]["min"][0].as_f64().unwrap() - 110.0).abs() < 1e-8);
}

#[test]
fn redefining_fillet_style_keeps_primary_parameter_and_downstream_references() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    run(&mut s, "solid.fillet", json!({"edges": [[10, 10, 30]], "radius": 2, "name": "Round"}));
    let parameter = s.doc.find_feature("Round").unwrap().param_names[0].clone();
    run(&mut s, "solid.box", json!({"length": parameter, "width": 2, "height": 2, "corner": [50, 0, 0], "name": "Dependent"}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Round", "command": "solid.fillet", "params": {"edges": [[10, 10, 30]], "radius": 3, "type": "chord"}}),
    );
    assert_eq!(s.doc.find_feature("Round").unwrap().param_names[0], parameter);
    let dependent = s.doc.find_feature("Dependent").unwrap().id;
    assert!(s.model.result(dependent).unwrap().error.is_none());
    let value = run(&mut s, "expr.evaluate", json!({"expression": parameter}));
    assert_eq!(value["value"], 3.0);
}

#[test]
fn inserting_variable_fillet_control_preserves_parameters_at_existing_positions() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 30}));
    run(
        &mut s,
        "solid.fillet",
        json!({"edges": [[10, 10, 30]], "radius": 2, "radius2": 2, "type": "variable", "start": [10, 0, 30], "mid": [[0.25, 3], [0.75, 4]], "name": "Round"}),
    );
    let parameters = s.doc.find_feature("Round").unwrap().param_names.clone();
    run(&mut s, "solid.box", json!({"length": parameters[2], "width": parameters[3], "height": 2, "corner": [50, 0, 0], "name": "Dependent"}));
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Round", "command": "solid.fillet", "params": {"edges": [[10, 10, 30]], "radius": 2, "radius2": 2, "type": "variable", "start": [10, 0, 30], "mid": [[0.1, 2.5], [0.25, 3], [0.75, 4]]}}),
    );
    let edited = s.doc.find_feature("Round").unwrap();
    assert_eq!(edited.param_names[0..2], parameters[0..2]);
    assert_eq!(edited.param_names[3..5], parameters[2..4]);
    assert!(!parameters.contains(&edited.param_names[2]), "the inserted control needs a new dimension");
    let dependent = s.doc.find_feature("Dependent").unwrap().id;
    assert!(s.model.result(dependent).unwrap().error.is_none());
    let length = run(&mut s, "expr.evaluate", json!({"expression": parameters[2]}));
    let width = run(&mut s, "expr.evaluate", json!({"expression": parameters[3]}));
    assert_eq!(length["value"], 3.0);
    assert_eq!(width["value"], 4.0);
}

#[test]
fn timeline_folders_are_command_backed_saved_undoable_and_follow_feature_ids() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base"}));
    run(&mut s, "solid.fillet", json!({"edges": [[10, 5, 10]], "radius": 1, "name": "Round"}));
    let base = s.doc.find_feature("Base").unwrap().id;
    let round = s.doc.find_feature("Round").unwrap().id;

    let made = run(&mut s, "timeline.group", json!({"name": "Housing", "features": ["Base", round]}));
    let group = made["group"].as_u64().unwrap();
    assert_eq!(s.doc.timeline_groups[0].features, vec![base, round]);
    run(&mut s, "timeline.rename", json!({"feature": base, "name": "Foundation"}));
    assert_eq!(s.doc.timeline_groups[0].features, vec![base, round], "folders follow stable ids, not names");
    run(&mut s, "timeline.rename_group", json!({"group": group, "name": "Main housing"}));
    run(&mut s, "timeline.move_to_group", json!({"features": [round], "group": null}));
    assert_eq!(s.doc.timeline_groups[0].features, vec![base]);

    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.timeline_groups, s.doc.timeline_groups);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.timeline_groups[0].features, vec![base, round]);
    run(&mut s, "timeline.delete", json!({"features": [base]}));
    assert_eq!(s.doc.timeline_groups[0].features, vec![round], "deleted features leave folders cleanly");
    run(&mut s, "timeline.ungroup", json!({"group": group}));
    assert!(s.doc.timeline_groups.is_empty());
}

#[test]
fn timeline_reorders_selected_features_as_one_block_and_removes_folders_in_one_step() {
    let mut s = Session::default();
    for (name, body, x) in [("First", "FirstBody", 0), ("Second", "SecondBody", 20), ("Third", "ThirdBody", 40)] {
        run(
            &mut s,
            "solid.box",
            json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
        );
    }
    let names = |session: &Session| session.doc.features.iter().map(|feature| feature.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&s), vec!["First", "Second", "Third"]);

    run(&mut s, "timeline.reorder_many", json!({"features": ["First", "Third"], "position": 3}));
    assert_eq!(names(&s), vec!["Second", "First", "Third"], "selected features keep their relative order");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), vec!["First", "Second", "Third"]);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(names(&s), vec!["Second", "First", "Third"]);
    run(&mut s, "edit.undo", json!({}));

    let first = run(&mut s, "timeline.group", json!({"name": "One", "features": ["First"]}))["group"].as_u64().unwrap();
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.timeline_groups.is_empty(), "creating a folder is undoable");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.timeline_groups.first().map(|group| group.id), Some(first));
    let second = run(&mut s, "timeline.group", json!({"name": "Two", "features": ["Second"]}))["group"].as_u64().unwrap();
    run(&mut s, "timeline.ungroup", json!({"groups": [first, second]}));
    assert!(s.doc.timeline_groups.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.timeline_groups.len(), 2, "removing several folders is one undoable command");
    run(&mut s, "edit.redo", json!({}));
    assert!(s.doc.timeline_groups.is_empty(), "removing folders is redoable");
}

#[test]
fn timeline_folders_refuse_unknown_non_contiguous_and_split_contents() {
    let mut s = Session::default();
    for (name, body, x) in [("First", "FirstBody", 0), ("Second", "SecondBody", 20), ("Third", "ThirdBody", 40)] {
        run(
            &mut s,
            "solid.box",
            json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
        );
    }
    let before = s.doc.clone();
    let error = s.execute("timeline.group", &json!({"features": ["First", "Third"]})).unwrap_err().to_string();
    assert!(error.contains("contiguous"), "{error}");
    assert_eq!(s.doc, before);

    let group = run(&mut s, "timeline.group", json!({"name": "Pair", "features": ["First", "Second"]}))["group"].as_u64().unwrap();
    let before = s.doc.clone();
    let error = s.execute("timeline.reorder_many", &json!({"features": ["First"], "position": 3})).unwrap_err().to_string();
    assert!(error.contains("contiguous"), "{error}");
    assert_eq!(s.doc, before, "a refused reorder must not split the folder");

    for params in [json!({"features": [999_u64], "group": group}), json!({"groups": [999_u64], "group": group})] {
        let error = s.execute("timeline.move_to_group", &params).unwrap_err().to_string();
        assert!(error.contains("no feature") || error.contains("no timeline folder"), "{error}");
    }

    let mut damaged: Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    let first = s.doc.find_feature("First").unwrap().id;
    let third = s.doc.find_feature("Third").unwrap().id;
    damaged["timeline_groups"] = json!([{"id": 1, "name": "Broken", "features": [first, third]}]);
    let error = solidcraft_doc::Document::from_json(&damaged.to_string()).unwrap_err().to_string();
    assert!(error.contains("not contiguous"), "{error}");
}

#[test]
fn deleted_timeline_group_ids_are_not_reused_after_reopening() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Block"}));
    let params = json!({"features": ["Block"]});
    let first = run(&mut s, "timeline.group", params.clone())["group"].as_u64().unwrap();
    run(&mut s, "timeline.ungroup", json!({"group": first}));
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    *s.doc_mut() = doc;
    let second = run(&mut s, "timeline.group", params)["group"].as_u64().unwrap();
    assert!(second > first, "deleted folder {first} was reused as {second}");
    assert!(s.execute("timeline.rename_group", &json!({"group": first, "name": "Stale reference"})).is_err());
}

#[test]
fn timeline_move_many_is_atomic_and_one_undo_step() {
    let mut s = Session::default();
    for (name, body, x) in [("First", "FirstBody", 0), ("Second", "SecondBody", 20), ("Third", "ThirdBody", 40)] {
        run(
            &mut s,
            "solid.box",
            json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name, "body_name": body}),
        );
    }
    let group = run(&mut s, "timeline.group", json!({"name": "Pair", "features": ["First"]}))["group"].as_u64().unwrap();
    let before = (*s.doc).clone();
    run(&mut s, "timeline.move_many", json!({"features": ["Third"], "group": group, "position": 1}));
    assert_eq!(s.doc.features.iter().map(|feature| feature.name.as_str()).collect::<Vec<_>>(), vec!["First", "Third", "Second"]);
    assert_eq!(s.doc.timeline_groups[0].features.len(), 2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*s.doc, before, "one undo restores both order and folder membership");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.doc.timeline_groups[0].features.len(), 2);

    let child = run(&mut s, "timeline.group", json!({"name": "Child", "features": ["Second"]}))["group"].as_u64().unwrap();
    let before_parent = (*s.doc).clone();
    let parent = run(&mut s, "timeline.group", json!({"name": "Parent", "groups": [group, child]}))["group"].as_u64().unwrap();
    assert!(s.doc.timeline_groups.iter().filter(|folder| folder.parent == Some(parent)).count() == 2);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*s.doc, before_parent, "creating a parent for selected folders is one undo step");
}

/// `active` moves features across the history marker together with the reorder: one undo step
/// restores both the order and the marker. Before, moving a feature more than one step behind the
//  history marker would need two undo steps: one to retore the order and one to move the marker
#[test]
fn timeline_moves_across_the_marker_are_one_undo_step() {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20), ("C", 40), ("D", 60)] {
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "operation": "new_body", "name": name}));
    }
    run(&mut s, "timeline.roll_to", json!({"position": 2}));
    let names = |s: &Session| s.doc.features.iter().map(|feature| feature.name.clone()).collect::<Vec<_>>();
    let before = (*s.doc).clone();

    // A B | C D: B moves between C and D, rolled back; C stays rolled back.
    run(&mut s, "timeline.move_many", json!({"features": ["B"], "position": 3, "active": false}));
    assert_eq!((names(&s), s.doc.marker), (vec!["A".into(), "C".into(), "B".into(), "D".into()], Some(1)));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*s.doc, before, "one undo restores both order and marker");

    // B stays in place and only crosses the marker; and back again with `timeline.reorder`.
    run(&mut s, "timeline.move_many", json!({"features": ["B"], "position": 2, "active": false}));
    assert_eq!((names(&s), s.doc.marker), (vec!["A".into(), "B".into(), "C".into(), "D".into()], Some(1)));
    run(&mut s, "timeline.reorder", json!({"feature": "B", "position": 1, "active": true}));
    assert_eq!(s.doc.marker, Some(2));
    run(&mut s, "timeline.reorder", json!({"feature": "C", "position": 2, "active": true}));
    run(&mut s, "timeline.reorder", json!({"feature": "D", "position": 3, "active": true}));
    assert_eq!(s.doc.marker, None, "every feature active again");

    let undo = s.undo.len();
    let error = s.execute("timeline.reorder", &json!({"feature": "A", "position": 0, "active": false})).unwrap_err().to_string();
    assert!(error.contains("behind the history marker"), "{error}");
    assert_eq!(s.undo.len(), undo, "a refused move adds no undo step");
}

/// #5962: a reorder that breaks a feature is refused unless `allow_errors`; with it the move is
/// made, the reply lists what broke, and one undo puts the order back. For both reorder commands.
#[test]
fn reorder_with_allow_errors_is_made_and_lists_what_broke() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base"}));
    run(&mut s, "solid.fillet", json!({"edges": [[10, 5, 10]], "radius": 1, "name": "Round"}));
    let order = |s: &Session| s.doc.features.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
    let before = s.doc.clone();
    for (cmd, params) in
        [("timeline.reorder", json!({"feature": "Round", "position": 0})), ("timeline.reorder_many", json!({"features": ["Round"], "position": 0}))]
    {
        let error = s.execute(cmd, &params).unwrap_err().to_string();
        assert!(error.contains("breaks Round"), "{cmd}: {error}");
        assert_eq!(s.doc, before, "{cmd}: refused, nothing moved");
        let mut anyway = params.clone();
        anyway["allow_errors"] = json!(true);
        let r = run(&mut s, cmd, anyway);
        assert_eq!(order(&s), ["Round", "Base"], "{cmd}");
        let broken = r["broken"].as_array().unwrap();
        assert_eq!(broken.len(), 1, "{cmd}: {r}");
        assert_eq!(broken[0]["feature"], "Round", "{cmd}: {r}");
        assert!(broken[0]["error"].as_str().is_some_and(|e| !e.is_empty()), "{cmd}: {r}");
        run(&mut s, "edit.undo", json!({}));
        assert_eq!(s.doc, before, "{cmd}: one undo puts the order back");
    }
    // A move that breaks nothing says so, and refuses anything but a boolean.
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0], "operation": "new", "name": "Other"}));
    let r = run(&mut s, "timeline.reorder_many", json!({"features": ["Other"], "position": 0, "allow_errors": true}));
    assert_eq!(r["broken"], json!([]));
    assert!(s.execute("timeline.reorder", &json!({"feature": "Other", "position": 2, "allow_errors": "yes"})).is_err());
    // `reorder_many` takes `active` too: the moved feature ends up behind the history marker.
    run(&mut s, "timeline.reorder_many", json!({"features": ["Other"], "position": 3, "active": false}));
    assert_eq!(order(&s).last().map(String::as_str), Some("Other"));
    assert_eq!(s.doc.marker, Some(2));
}

#[test]
fn failed_timeline_move_many_keeps_folder_membership_and_order() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base"}));
    run(&mut s, "solid.fillet", json!({"edges": [[10, 5, 10]], "radius": 1, "name": "Round"}));
    let group = run(&mut s, "timeline.group", json!({"name": "Destination"}))["group"].as_u64().unwrap();
    let before = s.doc.clone();
    let undo = s.undo.len();
    let error = s.execute("timeline.move_many", &json!({"features": ["Base"], "group": group, "position": 2})).unwrap_err().to_string();
    assert!(error.contains("breaks"), "{error}");
    assert_eq!(s.doc, before);
    assert_eq!(s.undo.len(), undo, "a refused atomic move adds no undo step");
}

#[test]
fn timeline_folders_can_nest_and_reject_cycles() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base"}));
    let root = run(&mut s, "timeline.group", json!({"name": "Root"}))["group"].as_u64().unwrap();
    let child = run(&mut s, "timeline.group", json!({"name": "Child", "parent": root}))["group"].as_u64().unwrap();
    assert_eq!(s.doc.timeline_groups.iter().find(|group| group.id == child).unwrap().parent, Some(root));
    run(&mut s, "timeline.move_to_group", json!({"groups": [child], "group": null}));
    assert_eq!(s.doc.timeline_groups.iter().find(|group| group.id == child).unwrap().parent, None);
    run(&mut s, "timeline.move_to_group", json!({"groups": [child], "group": root}));
    let error = s.execute("timeline.move_to_group", &json!({"groups": [root], "group": child})).unwrap_err().to_string();
    assert!(error.contains("itself") || error.contains("descendant"), "{error}");
    let before = s.doc.clone();
    let error = s.execute("timeline.move_many", &json!({"groups": [root], "group": child, "position": 0})).unwrap_err().to_string();
    assert!(error.contains("itself") || error.contains("descendant"), "{error}");
    assert_eq!(s.doc, before);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.timeline_groups, s.doc.timeline_groups);
    run(&mut s, "timeline.ungroup", json!({"group": root}));
    assert_eq!(s.doc.timeline_groups.iter().find(|group| group.id == child).unwrap().parent, None, "deleting a folder promotes its children");
}

/// Four separate boxes, `First`…`Fourth`, for the timeline folder tests.
fn four_boxes() -> Session {
    let mut s = Session::default();
    for (i, name) in ["First", "Second", "Third", "Fourth"].into_iter().enumerate() {
        let corner = [20 * i, 0, 0];
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": corner, "operation": "new_body", "name": name}));
    }
    s
}

fn feature_names(s: &Session) -> Vec<String> {
    s.doc.features.iter().map(|feature| feature.name.clone()).collect()
}

#[test]
fn timeline_keys_distinguish_numeric_names_from_ids() {
    let mut s = four_boxes();
    let id = s.doc.find_feature("Second").unwrap().id;
    let named = s.doc.find_feature("Third").unwrap().id;
    let key = id.to_string();
    run(&mut s, "timeline.rename", json!({"feature": named, "name": key}));

    assert_eq!(run(&mut s, "timeline.inspect", json!({"feature": key}))["id"], named);
    assert_eq!(run(&mut s, "timeline.inspect", json!({"feature": id}))["id"], id);
    assert_eq!(run(&mut s, "timeline.inspect", json!({"feature": named.to_string()}))["id"], named);
    assert_eq!(run(&mut s, "timeline.copy", json!({"features": [key, id]}))["copied"], json!(["Second", key]));

    // Resolve the whole request before deleting: a repeated name must not turn into an id
    // after the first deletion removes the feature with that name.
    assert_eq!(run(&mut s, "timeline.delete", json!({"features": [key, key]}))["deleted"], json!([named]));
    assert!(s.doc.feature(id).is_some());
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "selection.delete", json!({"items": [Sel::Feature { id }]}));
    assert!(s.doc.feature(id).is_none());
    assert!(s.doc.feature(named).is_some());
}

#[test]
fn timeline_keys_prefer_exact_case_and_refuse_an_ambiguous_fallback() {
    let mut s = four_boxes();
    let second = s.doc.find_feature("Second").unwrap().id;
    let third = s.doc.find_feature("Third").unwrap().id;
    run(&mut s, "timeline.rename", json!({"feature": third, "name": "second"}));

    let before = s.doc.clone();
    let error = s.execute("timeline.delete", &json!({"features": ["First", "SECOND"]})).unwrap_err().to_string();
    assert!(error.contains("ambiguous"), "{error}");
    assert_eq!(s.doc, before);
    assert_eq!(run(&mut s, "timeline.inspect", json!({"feature": "Second"}))["id"], second);
    assert_eq!(run(&mut s, "timeline.suppress", json!({"feature": "second"}))["feature"], third);
    assert_eq!(run(&mut s, "timeline.inspect", json!({"feature": "FIRST"}))["name"], "First");
}

/// `timeline.delete {groups}` deletes a folder with its features and nested folders in one undo
/// step; other folders and features stay, and `timeline.ungroup` still keeps the features (#7219).
#[test]
fn timeline_delete_takes_a_folder_with_its_features_and_nested_folders() {
    let mut s = four_boxes();
    let outer = run(&mut s, "timeline.group", json!({"name": "Outer", "features": ["Second"]}))["group"].as_u64().unwrap();
    let inner = run(&mut s, "timeline.group", json!({"name": "Inner", "features": ["Third"], "parent": outer}))["group"].as_u64().unwrap();
    let keep = run(&mut s, "timeline.group", json!({"name": "Keep", "features": ["Fourth"]}))["group"].as_u64().unwrap();
    let before = s.doc.clone();

    let out = run(&mut s, "timeline.delete", json!({"groups": [outer]}));
    assert_eq!(feature_names(&s), ["First", "Fourth"]);
    assert_eq!(out["groups"], json!([outer, inner]), "{out}");
    assert_eq!(s.doc.timeline_groups.iter().map(|group| group.id).collect::<Vec<_>>(), [keep], "the nested folder goes too");
    assert_eq!(s.model.state().bodies.len(), 2);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.timeline_groups, s.doc.timeline_groups);

    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.features, before.features, "one undo step brings the features back");
    assert_eq!(s.doc.timeline_groups, before.timeline_groups, "and the folders, nested as they were");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(feature_names(&s), ["First", "Fourth"]);
    run(&mut s, "edit.undo", json!({}));

    // A folder and a feature outside it in one delete; an empty folder goes on its own.
    run(&mut s, "timeline.delete", json!({"groups": [inner], "features": ["First"]}));
    assert_eq!(feature_names(&s), ["Second", "Fourth"]);
    assert_eq!(s.doc.timeline_groups.iter().map(|group| group.name.as_str()).collect::<Vec<_>>(), ["Outer", "Keep"]);
    let empty = run(&mut s, "timeline.group", json!({"name": "Empty"}))["group"].as_u64().unwrap();
    assert_eq!(run(&mut s, "timeline.delete", json!({"groups": [empty]}))["deleted"], json!([]));
    assert!(!s.doc.timeline_groups.iter().any(|group| group.id == empty));

    let before = s.doc.clone();
    for bad in [json!({"groups": [999_999]}), json!({"groups": ["Outer"]}), json!({"groups": 3}), json!({"groups": []})] {
        assert!(s.execute("timeline.delete", &bad).is_err(), "{bad}");
        assert_eq!(s.doc, before, "{bad}: a refused delete changes nothing");
    }
}

/// `timeline.suppress {groups}` suppresses every feature of a folder and its nested folders in
/// one undo step, toggles like several features do, and works with the marker inside the
/// folder (#7219).
#[test]
fn timeline_suppress_takes_a_folder_with_its_nested_folders() {
    let mut s = four_boxes();
    let outer = run(&mut s, "timeline.group", json!({"name": "Outer", "features": ["Second"]}))["group"].as_u64().unwrap();
    run(&mut s, "timeline.group", json!({"name": "Inner", "features": ["Third"], "parent": outer}));
    let suppressed = |s: &Session| s.doc.features.iter().filter(|feature| feature.suppressed).map(|feature| feature.name.clone()).collect::<Vec<_>>();

    let out = run(&mut s, "timeline.suppress", json!({"groups": [outer]}));
    assert_eq!(out["suppressed"], true);
    assert_eq!(out["groups"], json!([outer]));
    assert_eq!(suppressed(&s), ["Second", "Third"]);
    assert_eq!(s.model.state().bodies.len(), 2);
    let reopened = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(reopened.features, s.doc.features);
    run(&mut s, "edit.undo", json!({}));
    assert!(suppressed(&s).is_empty(), "one undo step");
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(suppressed(&s), ["Second", "Third"]);

    // Toggle: all suppressed, so all come back; with one of them active, all are suppressed.
    run(&mut s, "timeline.suppress", json!({"groups": [outer]}));
    assert!(suppressed(&s).is_empty());
    run(&mut s, "timeline.suppress", json!({"feature": "Third"}));
    run(&mut s, "timeline.suppress", json!({"groups": [outer]}));
    assert_eq!(suppressed(&s), ["Second", "Third"]);
    run(&mut s, "timeline.suppress", json!({"groups": [outer], "suppressed": false, "features": ["Fourth"]}));
    assert!(suppressed(&s).is_empty(), "an explicit state, with a feature outside the folder as well");

    // The marker between the folder's features: the folder is suppressed all the same, and the
    // marker stays where it was.
    run(&mut s, "timeline.roll_to", json!({"feature": "Second"}));
    assert_eq!(s.doc.marker, Some(2));
    run(&mut s, "timeline.suppress", json!({"groups": [outer], "suppressed": true}));
    assert_eq!(suppressed(&s), ["Second", "Third"]);
    assert_eq!(s.doc.marker, Some(2));
    assert_eq!(s.model.state().bodies.len(), 1, "only First is before the marker and not suppressed");

    let empty = run(&mut s, "timeline.group", json!({"name": "Empty"}))["group"].as_u64().unwrap();
    let before = s.doc.clone();
    for bad in [json!({"groups": [empty]}), json!({"groups": [999_999]}), json!({"groups": [outer], "suppressed": "yes"})] {
        assert!(s.execute("timeline.suppress", &bad).is_err(), "{bad}");
        assert_eq!(s.doc, before, "{bad}: a refused suppress changes nothing");
    }
}

/// A field the feature's definition doesn't have is refused with the fields it does have,
/// instead of reporting success and changing nothing (#1491).
#[test]
fn edit_refuses_unknown_fields() {
    let mut s = Session::default();
    run(&mut s, "construct.plane.offset", json!({"base": "XZ", "offset": 50, "name": "P"}));
    let e = s.execute("timeline.edit", &json!({"feature": "P", "set": {"offset": "-50"}})).unwrap_err().to_string();
    assert!(e.contains("unknown field `offset`") && e.contains("`plane.distance`"), "{e}");
    let e = s.execute("timeline.edit", &json!({"feature": "P", "set": {"plane": {"dist": "-50"}}})).unwrap_err().to_string();
    assert!(e.contains("unknown field `plane.dist`") && e.contains("`plane.distance`"), "{e}");
    let def = run(&mut s, "timeline.inspect", json!({"feature": "P"}))["definition"].clone();
    assert_eq!(def["plane"]["distance"], json!("50"), "nothing changed: {def}");
    // The field it does have edits as before.
    run(&mut s, "timeline.edit", json!({"feature": "P", "set": {"plane": {"distance": "-50"}}}));
    let def = run(&mut s, "timeline.inspect", json!({"feature": "P"}))["definition"].clone();
    assert_eq!(def["plane"]["distance"], json!("-50"), "{def}");

    // Fields the definition leaves out at their default are still fields: `combine: false`
    // and `bodies: []` aren't shown on a feature mirror, and set them as they are.
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "Base"}));
    run(&mut s, "solid.mirror", json!({"features": ["Base"], "plane": "YZ", "name": "Twin"}));
    run(&mut s, "timeline.edit", json!({"feature": "Twin", "set": {"combine": false, "bodies": []}}));
    run(&mut s, "timeline.edit", json!({"feature": "Twin", "set": {"combine": true}}));
    // An unknown field next to known ones refuses the whole edit.
    let e = s.execute("timeline.edit", &json!({"feature": "Twin", "set": {"combine": false, "flip": true}})).unwrap_err().to_string();
    assert!(e.contains("unknown field `flip`") && e.contains("`combine`") && e.contains("`plane.name`"), "{e}");
    let def = run(&mut s, "timeline.inspect", json!({"feature": "Twin"}))["definition"].clone();
    assert_eq!(def["combine"], json!(true), "{def}");
}

/// The check before a delete (`timeline.dependents`) evaluates on the session's model, so it
/// shares the evaluation cache (#2678): the delete that follows finds the features after the
/// deleted one already computed instead of computing them again. (What it reports is covered by
/// the timeline tests in `tests.rs`.)
#[test]
fn the_dependents_check_leaves_the_delete_its_results() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 60, "width": 20, "height": 10, "name": "Base"}));
    run(&mut s, "solid.box", json!({"corner": [5, 5, 10], "length": 10, "width": 10, "height": 5, "operation": "join", "name": "Boss"}));
    run(&mut s, "solid.cylinder", json!({"base": [40, 10, -1], "radius": 2, "height": 20, "operation": "cut", "name": "Hole"}));
    let report = run(&mut s, "timeline.dependents", json!({"feature": "Boss"}));
    assert_eq!(report, json!({"deleted_with_it": [], "would_fail": []}));
    let hits = |s: &Session| s.model.cache.lock().map(|c| c.hits).unwrap_or(0);
    let before = hits(&s);
    run(&mut s, "timeline.delete", json!({"features": ["Boss"]}));
    assert!(hits(&s) > before, "the delete takes the hole the check computed from the cache");
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - (12000.0 - std::f64::consts::PI * 4.0 * 10.0)).abs() < 1.0, "{volume}");
}

fn suppressed(s: &Session, name: &str) -> bool {
    s.doc.find_feature(name).unwrap().suppressed
}

/// timeline.suppress takes `features: [..]` as timeline.delete does, as one undo step; it used to
/// refuse it ("`feature` must be an id or name").
#[test]
fn suppress_takes_a_list_of_features_as_one_undo_step() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "name": "B"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "name": "C"}));
    let id = |s: &Session, name: &str| s.doc.find_feature(name).unwrap().id;
    let undo = s.undo.len();
    let a = id(&s, "A");
    let r = run(&mut s, "timeline.suppress", json!({"features": [a, "B"], "suppressed": true}));
    assert_eq!(r, json!({"features": [id(&s, "A"), id(&s, "B")], "suppressed": true}));
    assert!(suppressed(&s, "A") && suppressed(&s, "B") && !suppressed(&s, "C"));
    assert_eq!(s.undo.len(), undo + 1, "one undo step");
    assert_eq!(s.model.state().bodies.len(), 1);
    run(&mut s, "edit.undo", json!({}));
    assert!(!suppressed(&s, "A") && !suppressed(&s, "B"));
    assert_eq!(s.model.state().bodies.len(), 3);

    // Toggling several: all suppressed unless all already are, then all unsuppressed.
    run(&mut s, "timeline.suppress", json!({"feature": "A"}));
    run(&mut s, "timeline.suppress", json!({"features": ["A", "B", "C"]}));
    assert!(suppressed(&s, "A") && suppressed(&s, "B") && suppressed(&s, "C"));
    run(&mut s, "timeline.suppress", json!({"features": ["A", "B", "C"]}));
    assert!(!suppressed(&s, "A") && !suppressed(&s, "B") && !suppressed(&s, "C"));
    // `feature` and `features` together; a repeat counts once.
    let r = run(&mut s, "timeline.suppress", json!({"feature": "C", "features": ["C", "B"], "suppressed": true}));
    assert_eq!(r["features"], json!([id(&s, "C"), id(&s, "B")]), "{r}");
    assert!(!suppressed(&s, "A") && suppressed(&s, "B") && suppressed(&s, "C"));
    // A single `feature` replies as before.
    let r = run(&mut s, "timeline.suppress", json!({"feature": "C", "suppressed": false}));
    assert_eq!(r, json!({"feature": id(&s, "C"), "suppressed": false}));

    // An unknown feature in the list refuses the whole command, as do no features at all.
    let before = (*s.doc).clone();
    let undo = s.undo.len();
    for bad in [
        json!({"features": ["A", "Nope"], "suppressed": true}),
        json!({"features": []}),
        json!({}),
        json!({"features": [{}]}),
        json!({"features": ["A"], "suppressed": "yes"}),
    ] {
        assert!(s.execute("timeline.suppress", &bad).is_err(), "{bad}");
    }
    assert_eq!(*s.doc, before);
    assert_eq!(s.undo.len(), undo);
    // timeline.delete takes either.
    run(&mut s, "timeline.delete", json!({"feature": "C"}));
    run(&mut s, "timeline.delete", json!({"features": ["B"]}));
    assert!(s.doc.find_feature("B").is_none() && s.doc.find_feature("C").is_none());
}

/// #4302: suppressing a feature that made a counted body (`Body1`) leaves the counted bodies of
/// later features as they were: `Body2` used to become `Body1`, and a Move of `Body2` failed.
#[test]
fn suppressing_a_feature_keeps_later_counted_body_names() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "name": "B"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0], "name": "C"}));
    run(&mut s, "solid.move", json!({"bodies": ["Body2"], "translate": [0, 30, 0], "name": "Move"}));
    let bodies = |s: &Session| -> Vec<(String, String)> {
        let st = s.model.state();
        st.bodies.iter().map(|b| (b.name.clone(), s.doc.feature(b.feature).map(|f| f.name.clone()).unwrap_or_default())).collect()
    };
    let named = |pairs: &[(&str, &str)]| pairs.iter().map(|(b, f)| (b.to_string(), f.to_string())).collect::<Vec<_>>();
    let works = |s: &Session| s.doc.features.iter().all(|f| s.model.result(f.id).is_some_and(|r| r.error.is_none()));
    let moved = |s: &Session| crate::cmd::inspect::measure_json(s.model.state().body("Body2").unwrap())["bbox"]["min"].clone();
    assert_eq!(bodies(&s), named(&[("Body1", "A"), ("Body2", "B"), ("Body3", "C")]));
    let at = moved(&s);

    run(&mut s, "timeline.suppress", json!({"feature": "A", "suppressed": true}));
    assert_eq!(bodies(&s), named(&[("Body2", "B"), ("Body3", "C")]));
    assert!(works(&s), "the Move still finds Body2");
    assert_eq!(moved(&s), at, "and still moves B's body");

    // Back again: A takes its number back, the others keep theirs.
    run(&mut s, "timeline.suppress", json!({"feature": "A", "suppressed": false}));
    assert_eq!(bodies(&s), named(&[("Body1", "A"), ("Body2", "B"), ("Body3", "C")]));
    assert!(works(&s));

    // Several at once: the first one keeps the names of everything after it; undo restores all.
    run(&mut s, "timeline.suppress", json!({"features": ["B", "A"], "suppressed": true}));
    assert_eq!(bodies(&s), named(&[("Body3", "C")]));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(bodies(&s), named(&[("Body1", "A"), ("Body2", "B"), ("Body3", "C")]));
    assert!(works(&s));

    // A body made while A is suppressed does not take A's `Body1`, so it keeps its name once A is
    // back (it used to be `Body1`, then `Body4`).
    run(&mut s, "timeline.suppress", json!({"feature": "A", "suppressed": true}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "name": "D"}));
    assert_eq!(bodies(&s), named(&[("Body2", "B"), ("Body3", "C"), ("Body4", "D")]));
    // Also once the design is opened again with the model its file keeps, which does not keep
    // the names passed by: D computed again from a kept state is still `Body4`.
    let saved = s.model.saved(&s.doc).unwrap();
    let mut reopened = Session::opened(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap(), Some(&saved));
    run(&mut reopened, "timeline.edit", json!({"feature": "D", "set": {"length": "12"}}));
    assert_eq!(bodies(&reopened), named(&[("Body2", "B"), ("Body3", "C"), ("Body4", "D")]));
    run(&mut s, "timeline.suppress", json!({"feature": "A", "suppressed": false}));
    assert_eq!(bodies(&s), named(&[("Body1", "A"), ("Body2", "B"), ("Body3", "C"), ("Body4", "D")]));
    assert!(works(&s));
}

/// Counted body names are pinned when the body is made, as if the user had renamed it: deleting,
/// reordering or rolling back another feature leaves the other bodies' names alone, and a number
/// a deleted feature held is not handed to the next body.
#[test]
fn counted_body_names_stay_when_the_timeline_changes() {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20), ("C", 40)] {
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "name": name}));
    }
    run(&mut s, "solid.move", json!({"bodies": ["Body2"], "translate": [0, 30, 0], "name": "Move"}));
    let bodies = |s: &Session| -> Vec<(String, String)> {
        let st = s.model.state();
        st.bodies.iter().map(|b| (b.name.clone(), s.doc.feature(b.feature).map(|f| f.name.clone()).unwrap_or_default())).collect()
    };
    let named = |pairs: &[(&str, &str)]| pairs.iter().map(|(b, f)| (b.to_string(), f.to_string())).collect::<Vec<_>>();
    let works = |s: &Session| s.doc.features.iter().all(|f| s.model.result(f.id).is_some_and(|r| r.error.is_none()));
    // The names are stored on the features as soon as the bodies exist.
    assert_eq!(s.doc.find_feature("B").map(|f| f.body_names.clone()), Some(vec!["Body2".to_string()]));
    assert!(["Body1", "Body2", "Body3"].iter().all(|n| s.doc.reserved_body_names.contains(*n)));

    // Reordering A behind B: every body keeps its number, wherever it comes in the list.
    run(&mut s, "timeline.reorder", json!({"feature": "A", "position": 2, "active": true}));
    let mut reordered = bodies(&s);
    reordered.sort();
    assert_eq!(reordered, named(&[("Body1", "A"), ("Body2", "B"), ("Body3", "C")]));
    assert!(works(&s));
    run(&mut s, "edit.undo", json!({}));

    // Rolled back to before B and forward again.
    run(&mut s, "timeline.roll_to", json!({"position": 1}));
    run(&mut s, "timeline.roll_to", json!({"position": "end"}));
    assert_eq!(bodies(&s), named(&[("Body1", "A"), ("Body2", "B"), ("Body3", "C")]));

    // Deleting A: B and C keep their names, so the Move still finds Body2.
    run(&mut s, "timeline.delete", json!({"feature": "A"}));
    assert_eq!(bodies(&s), named(&[("Body2", "B"), ("Body3", "C")]));
    assert!(works(&s), "the Move still finds Body2");

    // The number A held is not handed out again.
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "name": "D"}));
    assert_eq!(bodies(&s), named(&[("Body2", "B"), ("Body3", "C"), ("Body4", "D")]));
    // Undo takes the box away, and the number back with it.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0], "name": "E"}));
    assert_eq!(bodies(&s), named(&[("Body2", "B"), ("Body3", "C"), ("Body4", "E")]));
}

/// Pinning reads each feature's bodies once and keeps them with the state it read them from;
/// the names it pins and reserves, and the fingerprints it leaves, are those of reading every
/// feature's bodies again: random boxes (named and counted), moves, suppressions, deletions,
/// reorders, undo and redo, against a session that forgets what it read before every command and
/// against fingerprinting the whole timeline again.
#[test]
fn pinning_with_the_bodies_it_read_before_pins_as_reading_them_again() {
    let (mut kept, mut fresh) = (Session::default(), Session::default());
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    for step in 0..60 {
        let features: Vec<String> = kept.doc.features.iter().map(|f| f.name.clone()).collect();
        let counted: Vec<String> = kept.model.state().bodies.iter().map(|b| b.name.clone()).collect();
        let pick = |names: &[String], k: usize| names.get(k % names.len().max(1)).cloned().unwrap_or_default();
        let x = 15 * step;
        let (id, params) = match next(9) {
            _ if features.len() < 3 => ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})),
            0 | 1 => ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]})),
            2 => ("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0], "body_name": format!("Named {step}")})),
            3 => ("solid.move", json!({"bodies": [pick(&counted, next(64))], "translate": [0, 5, 0]})),
            4 => ("timeline.suppress", json!({"feature": pick(&features, next(64)), "suppressed": next(2) == 0})),
            5 => ("timeline.delete", json!({"feature": pick(&features, next(64))})),
            6 => ("timeline.reorder", json!({"feature": pick(&features, next(64)), "position": next(features.len()), "active": true})),
            7 => ("edit.undo", json!({})),
            _ => ("edit.redo", json!({})),
        };
        fresh.made_bodies.clear();
        let (a, b) = (kept.execute(id, &params), fresh.execute(id, &params));
        assert_eq!(a.is_ok(), b.is_ok(), "step {step}: {id} {params}");
        assert_eq!(kept.doc.to_json(), fresh.doc.to_json(), "step {step}: {id} {params}");
        // The fingerprints match the design as fingerprinting the whole timeline again makes them
        // match it: evaluating it again reuses the same results (all, as a rule; the others are
        // computed or taken from the cache the copies share).
        let again = |model: &solidcraft_doc::Model| {
            let mut m = model.clone();
            m.evaluate(&kept.doc);
            m.last_recomputed + m.last_restored
        };
        let mut full = kept.model.clone();
        full.refingerprint(&kept.doc);
        assert_eq!(again(&kept.model), again(&full), "step {step}: {id} {params}");
        assert_eq!(again(&kept.model), again(&fresh.model), "step {step}: {id} {params}");
    }
    assert!(kept.doc.features.len() > 5 && !kept.doc.reserved_body_names.is_empty());
}

/// The value at a dotted path of a definition.
fn dotted<'a>(def: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(def, |v, k| &v[k])
}

/// The lowest Z of the design's first body.
fn low_z(s: &mut Session) -> f64 {
    let m = run(s, "inspect.measure", json!({}));
    let min = &m["bodies"][0]["bbox"]["min"];
    min[2].as_f64().or_else(|| min["z"].as_f64()).unwrap_or_else(|| panic!("{m}"))
}

/// timeline.edit takes the dotted keys its unknown-field message lists (`plane.distance`), and
/// numbers for expressions, as the feature commands do; both used to be refused.
#[test]
fn edit_takes_dotted_keys_and_numbers_for_expressions() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 5, "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5, "name": "Pad"}));
    let def = |s: &mut Session, f: &str| run(s, "timeline.inspect", json!({"feature": f}))["definition"].clone();

    run(&mut s, "timeline.edit", json!({"feature": "S", "set": {"plane.distance": "8 mm"}}));
    assert_eq!(def(&mut s, "S")["plane"]["distance"], json!("8 mm"));
    assert!((low_z(&mut s) - 8.0).abs() < 1e-9);
    // A bare number is mm, as in sketch.create's `offset`.
    run(&mut s, "timeline.edit", json!({"feature": "S", "set": {"plane.distance": 12}}));
    assert_eq!(def(&mut s, "S")["plane"]["distance"], json!("12"));
    assert!((low_z(&mut s) - 12.0).abs() < 1e-9);
    run(&mut s, "timeline.edit", json!({"feature": "S", "set": {"plane": {"distance": 3.5}}}));
    assert_eq!(def(&mut s, "S")["plane"]["distance"], json!("3.5"));
    // The nested form works as before.
    run(&mut s, "timeline.edit", json!({"feature": "S", "set": {"plane": {"distance": "4"}}}));
    assert!((low_z(&mut s) - 4.0).abs() < 1e-9);

    // Every field the unknown-field message lists is accepted as it is written there.
    let e = s.execute("timeline.edit", &json!({"feature": "S", "set": {"plane.dist": "1"}})).unwrap_err().to_string();
    assert!(e.contains("unknown field `plane.dist`") && e.contains("`plane.distance`"), "{e}");
    let e = s.execute("timeline.edit", &json!({"feature": "S", "set": {"nope": "1"}})).unwrap_err().to_string();
    let listed: Vec<&str> = e.split('`').skip(3).step_by(2).collect();
    assert!(listed.contains(&"plane.distance") && listed.contains(&"plane.type"), "{e}");
    // Not the sketch geometry, which timeline.inspect leaves out and the sketch commands edit.
    assert!(!listed.iter().any(|k| k.starts_with("sketch")), "{e}");
    let now = def(&mut s, "S");
    for key in listed {
        let value = dotted(&now, key).clone();
        run(&mut s, "timeline.edit", json!({"feature": "S", "set": {key: value}}));
    }
    assert_eq!(def(&mut s, "S"), now);

    // Numbers on an extrude, also for an expression the definition leaves out.
    run(&mut s, "timeline.edit", json!({"feature": "Pad", "set": {"extent.distance": 20, "extent": {"taper": 0}}}));
    let d = def(&mut s, "Pad");
    assert_eq!((&d["extent"]["distance"], &d["extent"]["taper"]), (&json!("20"), &json!("0")), "{d}");
    let volume = run(&mut s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!((volume - 2000.0).abs() < 1e-6, "{volume}");

    // Still refused: a field given twice, unknown dotted fields, a number for a name.
    let before = (*s.doc).clone();
    for (set, says) in [
        (json!({"plane.distance": "1", "plane": {"distance": "2"}}), "given twice"),
        (json!({"plane.wobble": "1"}), "unknown field `plane.wobble`"),
        (json!({"plane.distance.x": "1"}), "invalid feature"),
        (json!({"plane.base.name": 5}), "invalid feature"),
    ] {
        let e = s.execute("timeline.edit", &json!({"feature": "S", "set": set})).unwrap_err().to_string();
        assert!(e.contains(says), "{set}: {e}");
    }
    assert_eq!(*s.doc, before);
}

/// #3044: renaming a feature labels it and builds nothing, so it, its undo and its redo evaluate
/// nothing, and leave the model evaluating the renamed design gives (the result's, the sketch's
/// and the construction plane's names). A rename other features follow is evaluated.
#[test]
fn renaming_a_feature_evaluates_nothing() {
    let mut s = Session::default();
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 5, "name": "Shelf"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Outline"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 6]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 4, "name": "Plate"}));
    let labels = |s: &Session| {
        let st = s.model.state();
        let results: Vec<String> = s.model.results.iter().map(|r| r.name.clone()).collect();
        let sketches: Vec<String> = st.sketches.iter().map(|k| k.name.clone()).collect();
        let planes: Vec<String> = st.construct.iter().map(|c| c.name.clone()).collect();
        (results, sketches, planes)
    };
    for (feature, name) in [("Outline", "Profile"), ("Plate", "Base plate")] {
        s.evaluations = 0;
        run(&mut s, "timeline.rename", json!({"feature": feature, "name": name}));
        assert_eq!(s.evaluations, 0, "renaming {feature} evaluates nothing");
        let mut fresh = Session::new((*s.doc).clone());
        fresh.refresh();
        assert_eq!(labels(&s), labels(&fresh));
        s.refresh();
        assert_eq!(s.model.last_recomputed, 0, "the next evaluation reuses every result");
    }
    let renamed = labels(&s);
    s.evaluations = 0;
    run(&mut s, "edit.undo", json!({}));
    assert!(labels(&s).0.contains(&"Profile".to_string()) && labels(&s).0.contains(&"Plate".to_string()));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(labels(&s), renamed);
    assert_eq!(s.evaluations, 0, "undo and redo of a rename evaluate nothing");
    // A sketch on the plane names it by id (#1134): renaming the plane changes no reference,
    // so it now evaluates nothing — the name follows everywhere without a rebuild.
    run(&mut s, "sketch.create", json!({"plane": "Shelf", "name": "Top"}));
    run(&mut s, "sketch.finish", json!({}));
    s.evaluations = 0;
    run(&mut s, "timeline.rename", json!({"feature": "Shelf", "name": "Ledge"}));
    assert_eq!(s.evaluations, 0, "renaming a plane a sketch is built on evaluates nothing");
    assert!(labels(&s).2.contains(&"Ledge".to_string()));
    // A failed pattern's error quotes its source's name (the seed is keyed by id, #1134, and
    // the error is rendered from the source feature at evaluation): renaming the source is
    // evaluated, so the error names it as an evaluation does.
    run(&mut s, "solid.pattern.rectangular", json!({"features": ["Base plate"], "dir1": [0, 0, 1], "count1": 2, "spacing1": 10, "name": "Copies"}));
    let copies = s.doc.find_feature("Copies").unwrap().id;
    let top = s.doc.find_feature("Top").unwrap().id;
    if let Some(solidcraft_doc::FeatureKind::Pattern { features, .. }) = s.doc_mut().feature_mut(copies).map(|f| &mut f.kind) {
        *features = vec![top.into()];
    }
    s.refresh();
    let failed = |s: &Session| s.model.results.iter().filter_map(|r| r.error.clone()).collect::<Vec<_>>();
    assert!(failed(&s).iter().any(|e| e.contains("Top")), "{:?}", failed(&s));
    s.evaluations = 0;
    run(&mut s, "timeline.rename", json!({"feature": "Top", "name": "Lid sketch"}));
    assert!(s.evaluations > 0, "a later error quotes the old name");
    assert!(failed(&s).iter().any(|e| e.contains("Lid sketch")), "{:?}", failed(&s));
    let mut fresh = Session::new((*s.doc).clone());
    fresh.refresh();
    assert_eq!(failed(&s), failed(&fresh));
}

/// #4353: a construction plane is also named outside the features: by a sketch's projection of
/// it, a canvas placed on it and the hidden list. Renaming it renames it there too, and undo
/// takes the old name back.
#[test]
fn renaming_a_plane_keeps_its_projections_canvases_and_hidden_state() {
    let mut s = Session::default();
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 20, "name": "Shelf"}));
    run(&mut s, "construct.plane.offset", json!({"base": "YZ", "offset": -30, "name": "Guide"}));
    run(&mut s, "browser.visibility", json!({"items": ["Guide"], "visible": false}));
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend([0, 0, 0, 4, 0, 0, 0, 2, 8, 6, 0, 0, 0, 0, 0, 0, 0]);
    run(&mut s, "canvas.insert", json!({"data": solidcraft_doc::canvas::base64_encode(&png), "plane": "Shelf", "width": 40}));
    run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "Profile"}));
    run(&mut s, "sketch.project", json!({"refs": [{"plane": "Shelf"}]}));
    run(&mut s, "sketch.finish", json!({}));
    let plane_link = |s: &Session| match &s.doc.find_feature("Profile").unwrap().kind {
        solidcraft_doc::FeatureKind::Sketch { sketch, .. } => match &sketch.links[0].source {
            solidcraft_sketch::LinkSource::Plane { name } => name.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    let canvas_plane = |s: &Session| s.doc.canvases[0].plane.clone();
    let projected_z = |s: &mut Session| {
        let r = run(s, "sketch.inspect", json!({"sketch": "Profile"}));
        r["points"].as_array().unwrap().iter().find(|p| p["link"] == "j1").unwrap()["world"][2].as_f64().unwrap()
    };
    let warnings = |s: &Session| s.model.results.iter().filter_map(|r| r.warning.clone()).collect::<Vec<_>>();
    // The canvas keeps its plane by id (#1134): the rename leaves the reference alone.
    let shelf = s.doc.find_feature("Shelf").unwrap().id;
    run(&mut s, "select.set", json!({"items": [{"type": "plane", "name": "Shelf"}]}));
    run(&mut s, "timeline.rename", json!({"feature": "Shelf", "name": "Ledge"}));
    run(&mut s, "timeline.rename", json!({"feature": "Guide", "name": "Rail"}));
    assert_eq!(plane_link(&s), "Ledge");
    assert_eq!(canvas_plane(&s), solidcraft_doc::PlaneRef::Construction { feature: shelf.into() });
    assert_eq!(s.visibility.hidden_origin, ["Rail"]);
    assert_eq!(s.selection, [Sel::Plane { name: "Ledge".into() }]);
    assert!(warnings(&s).is_empty(), "{:?}", warnings(&s));
    // The projection follows the renamed plane.
    run(&mut s, "timeline.edit", json!({"feature": "Ledge", "set": {"plane.distance": "35"}}));
    assert!((projected_z(&mut s) - 35.0).abs() < 1e-9);
    assert!(warnings(&s).is_empty(), "{:?}", warnings(&s));
    // Undo takes the old name back everywhere in the design, and the sketch finds it again.
    for _ in 0..3 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(plane_link(&s), "Shelf");
    assert_eq!(canvas_plane(&s), solidcraft_doc::PlaneRef::Construction { feature: shelf.into() });
    assert_eq!(s.visibility.hidden_origin, ["Guide"], "the hidden plane stays hidden");
    assert!(warnings(&s).is_empty(), "{:?}", warnings(&s));
    assert!((projected_z(&mut s) - 20.0).abs() < 1e-9);
    run(&mut s, "edit.redo", json!({}));
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(s.visibility.hidden_origin, ["Rail"]);
    assert_eq!(plane_link(&s), "Ledge");
}

/// #6186: a reorder evaluates the design once. The model the move was checked on is the one the
/// session keeps, so setting the reordered design finds every result ready: nothing is
/// recomputed or taken from the cache again. The same for `timeline.reorder_many`, whose check
/// starts from the session's results rather than an empty model.
#[test]
fn a_reorder_keeps_the_model_it_was_checked_on() {
    let mut s = Session::default();
    for i in 0..4 {
        run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5 + i, "corner": [i * 10, 0, 0], "operation": "new"}));
    }
    let bodies = |s: &Session| s.model.state().bodies.len();
    run(&mut s, "timeline.reorder", json!({"feature": "Box2", "position": 0}));
    assert_eq!((s.model.last_recomputed, s.model.last_restored), (0, 0), "the checked model was kept");
    assert_eq!(s.doc.features.first().map(|f| f.name.as_str()), Some("Box2"));
    assert_eq!(bodies(&s), 4);
    run(&mut s, "timeline.reorder_many", json!({"features": ["Box4"], "position": 2}));
    assert_eq!((s.model.last_recomputed, s.model.last_restored), (0, 0), "the checked model was kept");
    assert_eq!(s.doc.features.get(2).map(|f| f.name.as_str()), Some("Box4"));
    // The kept model is the design's: evaluating it from scratch gives the same bodies.
    let mut fresh = solidcraft_doc::Model::new();
    fresh.evaluate(&s.doc);
    assert_eq!(fresh.state().bodies.len(), bodies(&s));
}

#[test]
fn select_same_look_uses_resolved_looks_without_document_history_edits_and_refuses_hostile_input() {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20), ("C", 40)] {
        run(&mut s, "solid.box", json!({"corner":[x,0,0],"length":10,"width":10,"height":10,"body_name":name}));
    }
    run(&mut s, "appearance.assign", json!({"bodies":["A","B"],"appearance":"ABS - Red"}));
    run(&mut s, "appearance.assign", json!({"bodies":["C"],"appearance":"ABS - Blue"}));
    run(&mut s, "appearance.assign", json!({"body":"C","faces":[[40,5,5]],"appearance":"ABS - Red"}));
    let doc = s.doc.clone();
    let history = (s.undo.len(), s.redo.len(), s.evaluations);
    run(&mut s, "select.same_look", json!({"body":"A"}));
    assert_eq!(s.selection.len(), 3);
    let point = solidcraft_geom::Vec3::new(40.0, 5.0, 5.0);
    let face = solidcraft_doc::appearance::face_index_at(s.world_state().body("C").unwrap(), point).unwrap();
    run(&mut s, "select.set", json!({"items":[Sel::Face {body:"C".into(),index:face,point}]}));
    run(&mut s, "select.same_look", json!({}));
    assert_eq!(s.selection.len(), 3);
    run(&mut s, "select.same_look", json!({"body":"C"}));
    assert_eq!(s.selection, [Sel::Body { name: "C".into() }]);
    let selected = s.selection.clone();
    // Session treats null parameters as the command's empty/default object.
    run(&mut s, "select.same_look", Value::Null);
    for params in [
        json!([]),
        json!({"body":"missing"}),
        json!({"body":"A","face":u64::MAX}),
        json!({"body":"A","face":-1}),
        json!({"body":1}),
        json!({"body":"A","extra":true}),
    ] {
        assert!(s.execute("select.same_look", &params).is_err(), "{params}");
        assert_eq!(s.selection, selected);
    }
    assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(history, (s.undo.len(), s.redo.len(), s.evaluations));
}

/// A body changed by a few hundred features in a row whose names nothing asks for, then
/// filleted: reordering or moving an earlier feature evaluates them all again, and the fillet
/// finds its edge by the names of the body before it, which are worked out from the body before
/// that, and so on back to the start. That walk recursed once per feature and overflowed the
/// window thread's stack (1 MB on Windows), ending the app (#7587, #7926). On a thread with that
/// stack, both moves are made.
#[test]
fn a_move_before_a_long_history_of_one_body_fits_a_window_threads_stack() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Peg"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 0], "body_name": "Other"}));
    for i in 0..300 {
        run(&mut s, "solid.move", json!({"bodies": ["Peg"], "translate": [0, 0, if i % 2 == 0 { 1 } else { -1 }]}));
    }
    run(&mut s, "solid.fillet", json!({"body": "Peg", "radius": 1, "edges": [[5, 0, 10]]}));
    let other = s.doc.features.get(1).map(|f| f.id).unwrap();
    let fillet = s.doc.features.last().map(|f| f.id).unwrap();
    let moved = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || {
            run(&mut s, "timeline.reorder", json!({"feature": other, "position": 0}));
            assert!(s.model.result(fillet).is_some_and(|r| r.error.is_none()), "the fillet is made after the reorder");
            run(&mut s, "timeline.move_many", json!({"features": [other], "position": 2}));
            assert_eq!(s.doc.features.get(1).map(|f| f.id), Some(other));
            assert!(s.model.result(fillet).is_some_and(|r| r.error.is_none()), "the fillet is made after the move");
        })
        .unwrap()
        .join();
    assert!(moved.is_ok());
}

#[test]
fn timeline_joint_folders_are_mixed_saved_and_undoable() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane":"XY", "name":"Profile"}));
    run(&mut s, "component.create", json!({"name":"Part"}));
    let joint = run(&mut s, "joint.as_built", json!({"type":"revolute", "a":0, "b":"Part", "name":"Hinge"}))["joint"].as_u64().unwrap();
    let folder = run(&mut s, "timeline.group", json!({"name":"Mixed", "features":["Profile"], "joints":["Hinge", joint]}))["group"].as_u64().unwrap();
    assert_eq!(s.doc.timeline_groups[0].joints, [joint]);
    assert_eq!(s.doc.timeline_groups[0].features.len(), 1);
    assert_eq!(run(&mut s, "timeline.inspect", json!({"group":folder}))["joints"], json!([joint]));
    let saved = s.doc.to_json();
    let reopened = solidcraft_doc::Document::from_json(&saved).unwrap();
    assert_eq!(reopened.to_json(), saved);
    let mut damaged: Value = serde_json::from_str(&saved).unwrap();
    damaged["timeline_groups"][0]["joints"] = json!([u64::MAX]);
    assert!(solidcraft_doc::Document::from_json(&damaged.to_string()).is_err());
    damaged["timeline_groups"][0]["joints"] = json!([joint, joint]);
    assert!(solidcraft_doc::Document::from_json(&damaged.to_string()).is_err());
    let destination = run(&mut s, "timeline.group", json!({"name":"Destination"}))["group"].as_u64().unwrap();
    run(&mut s, "timeline.move_to_group", json!({"joints":[joint], "group":destination}));
    assert!(s.doc.timeline_groups[0].joints.is_empty());
    assert_eq!(s.doc.timeline_groups[1].joints, [joint]);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.timeline_groups[0].joints, [joint]);
    // Both contents of a nested folder participate in the same toggle and undo.
    run(&mut s, "timeline.move_to_group", json!({"groups":[folder], "group":destination}));
    let before = s.doc.clone();
    run(&mut s, "timeline.suppress", json!({"groups":[destination]}));
    assert!(s.doc.features[0].suppressed && s.doc.assembly.joints[0].suppressed);
    run(&mut s, "timeline.suppress", json!({"groups":[destination]}));
    assert!(!s.doc.features[0].suppressed && !s.doc.assembly.joints[0].suppressed);
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.features[0].suppressed && s.doc.assembly.joints[0].suppressed);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*s.doc, *before);
    run(&mut s, "timeline.delete", json!({"groups":[destination]}));
    assert!(s.doc.features.is_empty() && s.doc.assembly.joints.is_empty() && s.doc.timeline_groups.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(*s.doc, *before);
    run(&mut s, "timeline.ungroup", json!({"group":folder}));
    assert_eq!(s.doc.timeline_groups[0].joints, [joint], "contents promote to the surviving parent");
    run(&mut s, "timeline.ungroup", json!({"group":destination}));
    assert!(s.doc.timeline_groups.is_empty());
    assert_eq!(s.doc.assembly.joints[0].id, joint);
    let folder = run(&mut s, "timeline.group", json!({"joints":vec![joint; 10_000]}))["group"].as_u64().unwrap();
    assert_eq!(s.doc.timeline_groups[0].joints, [joint], "the 10000-entry cap is inclusive and deduplicated");
    run(&mut s, "timeline.move_to_group", json!({"joints":["Hinge"], "group":null}));
    assert!(s.doc.timeline_groups[0].joints.is_empty());
    run(&mut s, "timeline.move_to_group", json!({"joints":["Hinge"], "group":folder}));
    run(&mut s, "timeline.suppress", json!({"groups":[folder]}));
    assert!(s.doc.assembly.joints[0].suppressed, "joint-only folders suppress too");
    run(&mut s, "joint.delete", json!({"joint":joint}));
    assert!(s.doc.timeline_groups[0].joints.is_empty());
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.timeline_groups[0].joints, [joint]);
    run(&mut s, "component.delete", json!({"component":"Part"}));
    assert!(s.doc.assembly.joints.is_empty());
    assert!(s.doc.timeline_groups[0].joints.is_empty());
    assert!(solidcraft_doc::Document::from_json(&s.doc.to_json()).is_ok());
}

#[test]
fn timeline_joint_folders_reject_hostile_joint_references_atomically() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name":"Part"}));
    let joint = run(&mut s, "joint.as_built", json!({"type":"rigid", "a":0, "b":"Part"}))["joint"].as_u64().unwrap();
    let folder = run(&mut s, "timeline.group", json!({}))["group"].as_u64().unwrap();
    for command in ["timeline.group", "timeline.move_to_group"] {
        for joints in
            [json!([u64::MAX]), json!([-1]), json!([1e100]), json!(["missing"]), json!([{}]), json!([null]), json!("bad"), json!(vec![joint; 10_001])]
        {
            let before = s.doc.to_json();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.execute(command, &json!({"joints":joints, "group":folder}))));
            let error = result.unwrap().unwrap_err();
            assert!(
                matches!(&error, crate::EngineError::BadParams { .. }),
                "{command} must reject invalid joint references without an internal panic: {error}"
            );
            assert_eq!(s.doc.to_json(), before);
        }
    }
}

/// A reference image is shared, not copied, by every edit after it: each undo state holds the
/// design as it was, and a copied image of up to 16 MiB per state would hold gigabytes.
#[test]
fn edits_after_a_canvas_share_its_image_with_every_undo_state() {
    let mut s = Session::default();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend([0, 0, 0, 4, 0, 0, 0, 2, 8, 6, 0, 0, 0, 0, 0, 0, 0]);
    png.resize(64 << 10, 0);
    run(&mut s, "canvas.insert", json!({"data": solidcraft_doc::canvas::base64_encode(&png), "plane": "XY", "width": 40}));
    for i in 0..20 {
        run(&mut s, "parameters.add", json!({"name": format!("p{i}"), "expression": "10 mm"}));
    }
    let image = s.doc.canvases[0].data.as_ptr();
    let copies = s.undo.iter().filter_map(|u| u.doc.canvases.first()).filter(|c| c.data.as_ptr() != image).count();
    assert_eq!(copies, 0, "undo states holding their own copy of the image");
    for _ in 0..20 {
        run(&mut s, "edit.undo", json!({}));
    }
    assert_eq!(s.doc.canvases[0].data.as_ptr(), image, "undo brings back the same image");
    assert_eq!(s.doc.canvases[0].bytes().as_deref(), Some(png.as_slice()));
}
