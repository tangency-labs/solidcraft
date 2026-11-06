use serde_json::{Value, json};

use super::{BodyFeaturesParams, BodyListParams, DescribeParams, ModelBodyFeatures, ModelEdges, ModelFaces, ModelThreads, ModelThreadsParams};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DescribeParams>(), &["command"]);
    assert_keys(input_schema_for::<BodyListParams>(), &["body"]);
    assert_keys(input_schema_for::<ModelThreadsParams>(), &[]);
    assert_keys(input_schema_for::<BodyFeaturesParams>(), &["body"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("engine.describe", json!({"command": 5})),
        ("engine.describe", json!({"command": null})),
        ("engine.describe", json!({"command": "solid.extrud"})),
        ("model.edges", json!({})),
        ("model.edges", json!({"body": "Nope"})),
        ("model.faces", json!({})),
        ("model.faces", json!({"body": "Nope"})),
        ("model.body_features", json!({"body": "Nope"})),
    ]);
}

/// #10942: a body's edges and faces and the design's threads reply as their output schemas say,
/// in Warn without warnings for good calls and with them for a misspelt key. A cylindrical face
/// carries `cylinder` and a folded sheet's face `sheet_face`; strict refuses a `body` that isn't
/// text, which the handlers read as no body.
#[test]
fn model_lists_reply_with_their_schemas() {
    let (edges, faces, threads) = (output_schema_for::<ModelEdges>(), output_schema_for::<ModelFaces>(), output_schema_for::<ModelThreads>());
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    run(&mut s, "solid.cylinder", json!({"radius": 4, "height": 20, "body_name": "Shaft"}));
    run(&mut s, "solid.thread", json!({"face": [4, 0, 10], "designation": "M8"}));
    run(&mut s, "sheet.manage_rules", json!({"name": "R1", "thickness": "1 mm", "bend_radius": "1 mm", "active": true}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [20, 0], "p1": [60, 30]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sheet.flange", json!({"sketch": "Base"}));
    let sheet = s.model.state().bodies.iter().map(|b| b.name.clone()).find(|n| n != "Shaft").unwrap_or_default();

    let r = run(&mut s, "model.edges", json!({"body": "Shaft"}));
    matches(&edges, &r, false);
    assert!(r["edges"].as_array().is_some_and(|e| !e.is_empty()), "{r}");
    let r = run(&mut s, "model.faces", json!({"body": "Shaft"}));
    matches(&faces, &r, false);
    assert!(r["faces"].as_array().into_iter().flatten().any(|f| f["cylinder"]["radius"].as_f64().is_some_and(|r| (r - 4.0).abs() < 1e-9)), "{r}");
    let r = run(&mut s, "model.faces", json!({"body": sheet}));
    matches(&faces, &r, false);
    assert!(r["faces"].as_array().into_iter().flatten().any(|f| f["sheet_face"].as_str().is_some_and(|n| n.starts_with("sheet:"))), "{r}");
    let r = run(&mut s, "model.threads", json!({}));
    matches(&threads, &r, false);
    assert_eq!((r["threads"][0]["designation"].as_str(), r["threads"][0]["hand"].as_str()), (Some("M8"), Some("right")), "{r}");

    matches(&edges, &run(&mut s, "model.edges", json!({"body": "Shaft", "bdy": 1})), true);
    matches(&faces, &run(&mut s, "model.faces", json!({"body": "Shaft", "bdy": 1})), true);
    matches(&threads, &run(&mut s, "model.threads", json!({"body": "Shaft"})), true);
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    strict.execute("solid.cylinder", &json!({"radius": 4, "height": 20, "body_name": "Shaft"})).unwrap_or_else(|e| panic!("{e}"));
    for id in ["model.edges", "model.faces"] {
        let error = strict.execute(id, &json!({"body": 5})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("`body` must be text"), "{id}: {error}");
    }
}

#[test]
fn describe_returns_both_schemas() {
    let mut s = Session::default();
    let d = s.execute("engine.describe", &json!({"command": "solid.box"})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d["id"], "solid.box");
    assert!(d["input"]["properties"].get("length").is_some());
    assert!(d["output"]["properties"].get("created_bodies").is_some());
    // Its own output schema lists the warnings warn mode can add.
    let own = s.execute("engine.describe", &json!({"command": "engine.describe"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(own["output"]["properties"].get("param_warnings").is_some(), "{own}");
    let d = s.execute("engine.describe", &json!({"command": "solid.revolve"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(d["input"].is_null() && d["output"].is_null(), "unconverted commands have no schemas yet");
    let e = s.execute("engine.describe", &json!({"command": "solid.extrud"})).err().map(|e| e.to_string());
    assert!(e.as_deref().is_some_and(|e| e.contains("did you mean `solid.extrude`")), "{e:?}");
}

/// #11945: each body lists the features that built it, in timeline order: its own box, the fillet
/// that changed it, and the cutting box a Combine consumed into it with that Combine. Another body
/// keeps only its own. Rolled back before the Combine, neither it nor the tool's box are listed.
#[test]
fn body_features_list_what_built_each_body() {
    let schema = output_schema_for::<ModelBodyFeatures>();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let id = |r: Value| r["feature"].as_u64().unwrap_or_else(|| panic!("{r}"));
    let a = id(run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "body_name": "A"})));
    let b = id(run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 0], "operation": "new", "body_name": "B"})));
    let fillet = id(run(&mut s, "solid.fillet", json!({"body": "A", "edges": [[10, 0, 10]], "radius": 1})));
    let tool = id(run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 20, "corner": [5, 5, -5], "operation": "new", "body_name": "T"})));
    let combine = id(run(&mut s, "solid.combine", json!({"target": "A", "tools": ["T"], "operation": "cut"})));

    let listed = |r: &Value, body: &str| -> Vec<u64> {
        let entry = r["bodies"].as_array().into_iter().flatten().find(|e| e["body"] == body).unwrap_or_else(|| panic!("no {body} in {r}"));
        entry["features"].as_array().into_iter().flatten().filter_map(|f| f["id"].as_u64()).collect()
    };
    let r = run(&mut s, "model.body_features", json!({}));
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    assert_eq!(listed(&r, "A"), vec![a, fillet, tool, combine], "{r}");
    assert_eq!(listed(&r, "B"), vec![b], "{r}");
    assert_eq!(r["bodies"].as_array().map(Vec::len), Some(2), "the consumed tool is gone: {r}");
    assert_eq!(r["bodies"][0]["features"][1]["name"], s.doc.feature(fillet).map(|f| f.name.clone()).unwrap_or_default().as_str());

    // One body, and the Session data the browser reads agree.
    let one = run(&mut s, "model.body_features", json!({"body": "B"}));
    assert_eq!((one["bodies"].as_array().map(Vec::len), listed(&one, "B")), (Some(1), vec![b]), "{one}");
    assert_eq!(s.body_features().get("A"), Some(&vec![a, fillet, tool, combine]));

    run(&mut s, "timeline.rollback", json!({"position": 3}));
    assert_eq!(s.body_features().get("A"), Some(&vec![a, fillet]));
    assert_eq!(s.body_features().get("B"), Some(&vec![b]));

    // A misspelt key warns; an unknown body is refused.
    let warned = run(&mut s, "model.body_features", json!({"bdy": "A"}));
    assert!(warned.get("param_warnings").is_some(), "{warned}");
    assert!(s.execute("model.body_features", &json!({"body": "Nope"})).is_err());
}
