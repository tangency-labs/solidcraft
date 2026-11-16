use serde_json::{Value, json};

use super::{IdList, JointList, SelectionDeleteParams, SelectionDeleteResult, SelectionItems};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};
use crate::{Sel, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn matches(schema: schemars::Schema, r: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SelectionDeleteParams>(),
        &["canvases", "components", "dry_run", "items", "joints", "occurrences", "point_clouds"],
    );
}

/// Strict gives the handler's messages for the calls it refuses for a key.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("selection.delete", json!({})),
        ("selection.delete", json!({"items": null})),
        ("selection.delete", json!({"items": "all"})),
        ("selection.delete", json!({"items": {"type": "body", "name": "Body1"}})),
        ("selection.delete", json!({"items": [{"type": "body"}]})),
        ("selection.delete", json!({"items": [{"type": "nothing"}]})),
        ("selection.delete", json!({"items": [5]})),
        ("selection.delete", json!({"items": [{"type": "feature", "id": -1}]})),
        ("selection.delete", json!({"items": [{"type": "face", "body": "B", "index": 0, "point": [1, 2]}]})),
        ("selection.delete", json!({"items": vec![json!({"type": "body", "name": "B"}); 10_001]})),
        ("selection.delete", json!({"joints": "J1"})),
        ("selection.delete", json!({"joints": [true]})),
        ("selection.delete", json!({"joints": [1.5]})),
        ("selection.delete", json!({"joints": [-1]})),
        ("selection.delete", json!({"joints": vec![json!(1); 10_001]})),
        ("selection.delete", json!({"joints": ["Nope"]})),
        ("selection.delete", json!({"joints": [99]})),
        ("selection.delete", json!({"components": ["Nope"]})),
        ("selection.delete", json!({"components": [0]})),
        ("selection.delete", json!({"occurrences": ["Nope"]})),
        ("selection.delete", json!({"canvases": ["Nope"]})),
        ("selection.delete", json!({"point_clouds": ["Nope"]})),
        ("selection.delete", json!({"items": [{"type": "feature", "id": 99999}]})),
        ("selection.delete", json!({"items": [{"type": "plane", "name": "XY"}]})),
    ]);
}

/// The types read each key as the handler does: what it takes they take, what it refuses (with
/// its message) they refuse.
#[test]
fn the_types_read_like_the_handler() {
    let items = |v: Value| serde_json::from_value::<SelectionItems>(v).map_err(|e| e.to_string());
    assert_eq!(items(json!([])).map(|i| i.0.len()), Ok(0));
    assert_eq!(items(json!([{"type": "feature", "id": 3}, {"type": "body", "name": "B"}])).map(|i| i.0.len()), Ok(2));
    assert_eq!(items(json!([{"type": "feature", "id": 3}])).map(|i| i.0), Ok(vec![Sel::Feature { id: 3 }]));
    assert!(items(json!("x")).is_err_and(|e| e.contains("`items` must be a list of selection items")));
    assert!(items(json!(null)).is_err());
    assert!(items(json!([{"type": "nothing"}])).is_err_and(|e| e.contains("bad item {\"type\":\"nothing\"}: ")));
    assert!(items(json!(vec![json!({"type": "body", "name": "B"}); 10_001])).is_err());
    assert!(items(json!(vec![json!({"type": "body", "name": "B"}); 10_000])).is_ok());

    let ids = |v: Value| serde_json::from_value::<IdList>(v).map(|l| l.0).ok();
    assert_eq!(ids(json!(["Plate", 3])), Some(vec!["Plate".to_string(), "3".to_string()]));
    assert_eq!(ids(json!([])), Some(vec![]));
    // What the handler ignores is ignored, not refused: a call that runs today is not warned about.
    assert_eq!(ids(json!("Plate")), Some(vec![]));
    assert_eq!(ids(json!(["a", true, -1, 1.5, null, 2])), Some(vec!["a".to_string(), "2".to_string()]));
    assert_eq!(ids(json!(vec![json!(1); 10_001])).map(|l| l.len()), Some(10_000));

    let joints = |v: Value| serde_json::from_value::<JointList>(v).map_err(|e| e.to_string());
    assert_eq!(joints(json!(["J1", 2])).map(|l| l.0), Ok(vec!["J1".to_string(), "2".to_string()]));
    assert!(joints(json!("J1")).is_err_and(|e| e.contains("`joints` must be a list of ids or names")));
    for bad in [json!([true]), json!([1.5]), json!([-1]), json!(vec![json!(1); 10_001])] {
        assert!(joints(bad).is_err_and(|e| e.contains("`joints` must contain at most 10000 ids or names")));
    }

    // Nothing is needed, and a `null` is a key left out (the handler says "nothing to delete").
    let params = |v: Value| serde_json::from_value::<SelectionDeleteParams>(v).is_ok();
    assert!(params(json!({})));
    assert!(params(
        json!({"items": null, "components": null, "occurrences": null, "canvases": null, "point_clouds": null, "joints": null, "dry_run": null})
    ));
    assert!(params(json!({"dry_run": true})));
    assert!(!params(json!({"dry_run": "yes"})));
}

/// The schema takes every kind of item the handler can read, and refuses what it can't.
#[test]
fn the_schema_takes_every_selection_item() {
    let schema = input_schema_for::<SelectionDeleteParams>();
    let schema = schema.as_value();
    let point = solidcore_record::Vec3::new(1.0, 2.0, 3.0);
    let all = [
        Sel::Occurrence { path: vec![1, 2] },
        Sel::Body { name: "B".into() },
        Sel::Edge { body: "B".into(), index: 0, point },
        Sel::Face { body: "B".into(), index: 1, point },
        Sel::Feature { id: 4 },
        Sel::SketchCurve { id: "c1".into(), sketch: None },
        Sel::SketchCurve { id: "c1".into(), sketch: Some(2) },
        Sel::SketchPoint { id: "p1".into() },
        Sel::SketchMidpoint { id: "c1".into() },
        Sel::ExternalSketchCurve { sketch: 2, id: "c1".into() },
        Sel::ExternalSketchPoint { sketch: 2, id: "p1".into() },
        Sel::SketchConstraint { id: "k1".into() },
        Sel::Profile { sketch: 2, index: 0 },
        Sel::Plane { name: "XY".into() },
        Sel::ConstructionPoint { name: "P".into() },
        Sel::Axis { name: "X".into() },
        Sel::Vertex { body: "B".into(), point },
    ];
    for item in all {
        let item = serde_json::to_value(&item).unwrap();
        assert!(conforms(schema, schema, &json!({"items": [item]})), "{item}");
        assert!(serde_json::from_value::<SelectionItems>(json!([item])).is_ok(), "{item}");
    }
    let takes = |p: Value, takes: bool| assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    takes(json!({}), true);
    takes(
        json!({"items": [], "components": ["A", 2], "occurrences": [1], "canvases": ["C"], "point_clouds": [3], "joints": ["J", 4], "dry_run": true}),
        true,
    );
    takes(json!({"items": [{"type": "nothing"}]}), false);
    takes(json!({"items": [{"type": "body"}]}), false);
    takes(json!({"items": "all"}), false);
    takes(json!({"joints": [true]}), false);
    takes(json!({"dry_run": "yes"}), false);
    takes(json!({"colour": 1}), false);
}

/// Calls that run today with a list the handler ignores get no warning and no refusal, in warn
/// and in strict.
#[test]
fn lists_the_handler_ignores_are_not_warned_about() {
    let mut s = part();
    let sketch = s.doc.features.first().map(|f| f.id).unwrap();
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        s.param_check = level;
        let p = json!({"items": [{"type": "feature", "id": sketch}], "dry_run": true, "components": "A", "canvases": [true, -1], "point_clouds": 5});
        let r = s.execute("selection.delete", &p).unwrap_or_else(|e| panic!("{e}"));
        assert!(r.get("param_warnings").is_none(), "{r}");
    }
}

/// A design with a sketch extruded into a body, a second box and an offset plane.
fn part() -> Session {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 20]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5, "corner": [50, 0, 0]}));
    run(&mut s, "construct.plane.offset", json!({"base": "XY", "offset": 10}));
    s
}

/// Three components, A grounded, with a joint from A to each of the others.
fn joint_assembly() -> Session {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for (name, x) in [("A", 0), ("B", 30), ("C", 60)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.ground", json!({"occurrence": "A:1", "grounded": true}));
    for (name, other, face) in [("Hinge", "B:1", [40, 10, 0]), ("Glue", "C:1", [70, 10, 0])] {
        let ends = json!({"a": {"occurrence": "A:1", "face": [10, 10, 10]}, "b": {"occurrence": other, "face": face}});
        let mut p = json!({"type": "revolute", "name": name});
        p.as_object_mut().unwrap().extend(ends.as_object().unwrap().clone());
        run(&mut s, "joint.create", p);
    }
    s
}

fn png() -> String {
    let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    b.extend(400u32.to_be_bytes());
    b.extend(200u32.to_be_bytes());
    b.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    solidcraft_doc::canvas::base64_encode(&b)
}

/// Every form of the command, in warn without warnings: the reply matches the output schema and
/// has the keys it had before (and no `param_warnings`).
#[test]
fn replies_match_the_output_schema() {
    let keys = |r: &Value| r.as_object().map(|o| o.keys().cloned().collect::<std::collections::BTreeSet<_>>()).unwrap_or_default();
    let expected: std::collections::BTreeSet<String> =
        ["bodies", "deleted", "deleted_joints", "skipped", "would_fail"].into_iter().map(String::from).collect();
    let check = |r: &Value| {
        matches(output_schema_for::<SelectionDeleteResult>(), r);
        assert_eq!(keys(r), expected, "{r}");
    };
    let mut s = part();
    let sketch = s.doc.features.first().map(|f| f.id).unwrap();
    // A dry run, then the real thing.
    let n = s.doc.features.len();
    let r = run(&mut s, "selection.delete", json!({"items": [{"type": "feature", "id": sketch}], "dry_run": true}));
    check(&r);
    assert_eq!(s.doc.features.len(), n);
    assert!(r["deleted"].as_array().is_some_and(|d| d.len() >= 2), "{r}");
    let done = run(&mut s, "selection.delete", json!({"items": [{"type": "feature", "id": sketch}]}));
    check(&done);
    assert_eq!(done, r, "the dry run says what the delete did");
    assert_eq!((&done["deleted_joints"], &done["skipped"]), (&json!([]), &json!([])), "{done}");
    // A body (a Remove feature), and an item that can't go, with its reason once.
    let body = s.model.state().bodies[0].name.clone();
    let plane = json!({"type": "plane", "name": "XY"});
    let r = run(&mut s, "selection.delete", json!({"items": [{"type": "body", "name": body}, plane, plane]}));
    check(&r);
    assert_eq!(r["bodies"], json!([body]), "{r}");
    assert_eq!(r["skipped"], json!(["the origin planes can't be deleted"]), "{r}");
    // A canvas by id, with the other keys `null`.
    run(&mut s, "canvas.insert", json!({"data": png()}));
    let canvas = s.doc.canvases[0].id;
    let r = run(&mut s, "selection.delete", json!({"canvases": [canvas], "items": null, "components": null, "joints": null, "dry_run": false}));
    check(&r);
    assert!(s.doc.canvases.is_empty());
    // A point cloud, by id.
    let dir = std::env::temp_dir().join(format!("solidcraft-delete-schema-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("two.xyz");
    std::fs::write(&path, "0 0 0\n1 0 0\n").unwrap();
    let cloud = run(&mut s, "pointcloud.insert", json!({"path": path}))["cloud"].clone();
    let r = run(&mut s, "selection.delete", json!({"point_clouds": [cloud]}));
    check(&r);
    let _ = std::fs::remove_dir_all(dir);

    // Joints, an occurrence of a component with others, and a component.
    let mut s = joint_assembly();
    let hinge = s.doc.assembly.joints[0].id;
    let r = run(&mut s, "selection.delete", json!({"joints": [hinge, "Glue"], "dry_run": true}));
    check(&r);
    assert_eq!(r["deleted_joints"].as_array().map(Vec::len), Some(2), "{r}");
    let r = run(&mut s, "selection.delete", json!({"joints": [hinge, "Glue"]}));
    check(&r);
    assert!(s.doc.assembly.joints.is_empty());
    let b2 = run(&mut s, "occurrence.copy", json!({"occurrence": "B:1", "translate": [0, 40, 0]}))["occurrence"].as_u64().unwrap();
    let r = run(&mut s, "selection.delete", json!({"occurrences": ["B:1"]}));
    check(&r);
    assert!(s.doc.occurrences.iter().any(|o| o.id == b2));
    let r = run(&mut s, "selection.delete", json!({"components": ["C"], "items": []}));
    check(&r);
    assert!(s.doc.components.iter().all(|c| c.name != "C"));
    // A selected occurrence, by its exact path.
    let o = s.doc.occurrences.iter().find(|o| o.name == "A:1").unwrap().id;
    let r = run(&mut s, "selection.delete", json!({"items": [{"type": "occurrence", "path": [o]}], "dry_run": true}));
    check(&r);
}

/// A misspelt key runs in warn and says so, the reply still matches the schema; strict refuses it.
#[test]
fn a_misspelt_key_is_reported_in_warn_and_refused_in_strict() {
    let mut s = part();
    let sketch = s.doc.features.first().map(|f| f.id).unwrap();
    let p = json!({"items": [{"type": "feature", "id": sketch}], "dryrun": true});
    let r = s.execute("selection.delete", &p).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| w.iter().any(|w| w.as_str().is_some_and(|w| w.contains("`dryrun`")))), "{r}");
    matches(output_schema_for::<SelectionDeleteResult>(), &r);
    assert!(s.doc.feature(sketch).is_none(), "the call still ran");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("selection.delete", &json!({"items": [], "dryrun": true})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`dryrun`"), "{error}");
}
