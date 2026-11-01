use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    EditInPlaceParams, FinishEditParams, MoveFeaturesParams, MoveSketchesToPartsParams, OccurrenceAlignParams, OccurrenceAlignResult,
    ReparentItemsParams,
};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<EditInPlaceParams>(), &["component", "path"]);
    assert_keys(input_schema_for::<FinishEditParams>(), &[]);
    assert_keys(
        input_schema_for::<OccurrenceAlignParams>(),
        &[
            "capture",
            "flip",
            "from",
            "from_face",
            "from_plane",
            "move_grounded",
            "occurrence",
            "occurrences",
            "path",
            "paths",
            "to",
            "to_face",
            "to_plane",
        ],
    );
    assert_keys(input_schema_for::<MoveFeaturesParams>(), &["component", "features"]);
    assert_keys(input_schema_for::<MoveSketchesToPartsParams>(), &["sketches"]);
    assert_keys(input_schema_for::<ReparentItemsParams>(), &["bodies", "component", "component_path", "features", "occurrences"]);
}

#[test]
fn editing_in_place_outside_the_window_only_checks_the_component() {
    let mut s = Session::default();
    let made = s.execute("component.create", &json!({"name": "Bracket"})).unwrap_or_else(|e| panic!("{e}"));
    let e = s.execute("component.edit_in_place", &json!({"component": made["component"]})).unwrap_err().to_string();
    assert!(e.contains("not linked"), "{e}");
    assert!(s.execute("component.edit_in_place", &json!({"component": "nothing"})).is_err());
    assert!(s.execute("component.edit_in_place", &json!({})).is_err(), "the component is required");
}

#[test]
fn finishing_an_edit_outside_the_window_changes_nothing() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let (doc, undo) = (s.doc.clone(), s.undo.len());
    let r = s.execute("component.finish_edit", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.as_object().is_some_and(serde_json::Map::is_empty), "{r}");
    assert!(Arc::ptr_eq(&doc, &s.doc) && s.undo.len() == undo);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<OccurrenceAlignResult>();
    let schema = schema.as_value();
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
    assert!(!required.contains(&"shared"), "`shared` is left out when nothing shared moved");

    let mut s = Session::default();
    let comp = s.execute("component.create", &json!({"name": "Extrusion"})).unwrap_or_else(|e| panic!("{e}"));
    let comp_id = comp["component"].as_u64().unwrap_or(0);
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 40})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("component.activate", &json!({"component": "root"})).unwrap_or_else(|e| panic!("{e}"));
    let copy = s.execute("occurrence.copy", &json!({"component": comp_id, "translate": [30, 0, 0]})).unwrap_or_else(|e| panic!("{e}"));
    let second = copy["occurrence"].as_u64().unwrap_or(0);

    s.param_check = ParamCheck::Warn;
    let r1 = s.execute("occurrence.align", &json!({"occurrence": second, "from": [30, 0, 0], "to": [10, 0, 0]})).unwrap_or_else(|e| panic!("{e}"));
    assert!(conforms(schema, schema, &r1), "{r1} doesn't match {schema}");

    let r2 = s
        .execute("occurrence.align", &json!({"paths": [[second]], "from": [10, 0, 0], "to": [20, 0, 0], "unknown_extra": 1}))
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(r2.get("param_warnings").is_some(), "{r2}");
    assert!(conforms(schema, schema, &r2), "{r2} doesn't match {schema}");
}

fn sketch_square(s: &mut Session) {
    let run = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"));
    run(s, "sketch.rectangle.two_point", json!({"p0":[1,1],"p1":[3,3]}));
    run(s, "sketch.finish", json!({}));
}

/// `component.move_sketches.to_parts` (#10218): sketches drawn on a part's face while the design
/// was active go to that part and stay where they are on the face; the others stay and say why;
/// it is one undo step.
#[test]
fn sketches_move_to_the_part_whose_face_they_are_on() {
    let mut s = Session::default();
    let run = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"));
    let part = run(&mut s, "component.create", json!({"name":"Part","parent":0,"grounded":false}));
    let (component, occurrence) = (part["component"].as_u64().unwrap(), part["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base"}));
    run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,0,0]}));
    run(&mut s, "component.activate", json!({"component":0}));
    let world = s.world_state();
    let base = world.body("Base").unwrap();
    let top = base.body.faces(0.01).unwrap().into_iter().max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
    let pick = json!({"body": base.name, "index": top.index, "face": [top.centroid.x, top.centroid.y, top.centroid.z], "path": [occurrence]});
    let on_face = run(&mut s, "sketch.create", json!({"name":"OnFace","plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    sketch_square(&mut s);
    let on_xy = run(&mut s, "sketch.create", json!({"name":"OnXY","plane":"XY"}))["sketch"].as_u64().unwrap();
    sketch_square(&mut s);
    let world_plane = |s: &Session| s.world_state().sketch(on_face).unwrap().plane;
    let before = world_plane(&s);
    assert_eq!(s.doc.feature(on_face).unwrap().component, 0);
    let doc_before = s.doc.to_json();

    let out = run(&mut s, "component.move_sketches.to_parts", json!({}));
    assert_eq!((out["moved"].as_u64(), out["stayed"].as_u64()), (Some(1), Some(1)), "{out}");
    assert_eq!(out["sketches"][0]["part"], "Part");
    assert_eq!(out["kept"][0]["name"], "OnXY");
    assert_eq!(out["kept"][0]["reason"], "not on a face");
    assert_eq!(s.doc.feature(on_face).unwrap().component, component);
    assert_eq!(s.doc.feature(on_xy).unwrap().component, 0);
    let solidcraft_doc::FeatureKind::Sketch { plane, .. } = &s.doc.feature(on_face).unwrap().kind else { panic!("not sketch") };
    assert_eq!(solidcraft_doc::face_anchor::plane_face_anchor(plane).unwrap().author_path, vec![occurrence]);
    assert!(s.model.result(on_face).unwrap().error.is_none());
    let after = world_plane(&s);
    assert!((after.origin - before.origin).len() < 1e-6 && after.normal().dot(before.normal()) > 1. - 1e-9, "{before:?} {after:?}");
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), doc_before, "one undo step");
    run(&mut s, "component.move_sketches.to_parts", json!({}));
    let again = run(&mut s, "component.move_sketches.to_parts", json!({"sketches":["OnFace"]}));
    assert_eq!((again["moved"].as_u64(), again["kept"][0]["reason"].as_str()), (Some(0), Some("already in its part")));
}
