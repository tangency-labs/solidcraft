use serde_json::{Value, json};

use super::{MAX_DETAIL, SelectionGetParams, SelectionGetResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SelectionGetParams>(), &["detail"]);
}

/// The schema, the params type and the handler on the same calls. They agree except where a JSON
/// schema cannot say what the handler does: `detail: null` means the default (true), and an
/// unknown key is refused by the contract rather than by the type it generates.
#[test]
fn the_schema_takes_what_the_handler_reads() {
    let root = input_schema_for::<SelectionGetParams>();
    let root = root.as_value().clone();
    let cases = [
        // (the call, whether the schema takes it, whether the params type reads it)
        (json!({}), true, true),
        (json!({"detail": true}), true, true),
        (json!({"detail": false}), true, true),
        (json!({"detail": null}), false, true),
        (json!({"detail": "yes"}), false, false),
        (json!({"detail": 1}), false, false),
        (json!({"detail": []}), false, false),
        (json!({"deatil": true}), false, true),
        (json!(7), false, false),
    ];
    for (p, schema_takes, type_reads) in cases {
        assert_eq!(conforms(&root, &root, &p), schema_takes, "{p}");
        assert_eq!(serde_json::from_value::<SelectionGetParams>(p.clone()).is_ok(), type_reads, "{p}");
    }
    // `detail: null` is the call the handler takes that the schema refuses: it means true.
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    assert_eq!(s.execute("selection.get", &json!({"detail": null})).unwrap()["items"], json!([]));
}

/// A `detail` the handler refuses is refused in strict with the handler's own message.
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("selection.get", json!({"detail": "yes"})),
        ("selection.get", json!({"detail": 1})),
        ("selection.get", json!({"detail": []})),
        ("selection.get", json!({"detail": {"on": true}})),
    ]);
}

/// What each item says it is.
fn kinds(items: &[Value]) -> Vec<&str> {
    items.iter().map(|i| i["kind"].as_str().unwrap_or_default()).collect()
}

/// Real selections of every kind, read with and without `detail`: the replies match the output
/// schema, name each item's kind, and resolve what they say they resolve. A misspelt key gets a
/// warning in Warn and is refused in Strict.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let schema = output_schema_for::<SelectionGetResult>();
    let schema = schema.as_value().clone();
    let run = |s: &mut Session, p: Value| {
        let r = s.execute("selection.get", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        assert!(conforms(&schema, &schema, &r), "{r} doesn't match {schema}");
        r
    };
    let select = |s: &mut Session, items: Vec<Value>| {
        let p = json!({"items": items});
        s.execute("select.set", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    };

    // A body, and the feature that made it: the reply names it, so the feature item is a real one.
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box1"})).unwrap();
    select(&mut s, vec![json!({"type": "body", "name": "Box1"})]);
    let r = run(&mut s, json!({}));
    assert_eq!(r["count"], 1);
    assert!(r.get("detail_limit").is_none(), "{r}");
    let body = &r["items"][0];
    assert_eq!(body["kind"], "body");
    assert!(body["volume_mm3"].is_number() && body["closed"] == true && body["visible"].is_boolean(), "{body:?}");
    let feature = body["feature"]["id"].as_u64().unwrap_or_default();
    assert!(feature > 0, "{body:?}");

    select(
        &mut s,
        vec![
            json!({"type": "body", "name": "Box1"}),
            json!({"type": "face", "body": "Box1", "index": 0}),
            json!({"type": "edge", "body": "Box1", "index": 0}),
            json!({"type": "vertex", "body": "Box1", "point": [0.0, 0.0, 0.0]}),
            json!({"type": "plane", "name": "XY"}),
            json!({"type": "axis", "name": "X"}),
            json!({"type": "feature", "id": feature}),
        ],
    );
    let r = run(&mut s, json!({}));
    assert_eq!(r["count"], 7);
    let items = r["items"].as_array().cloned().unwrap_or_default();
    assert_eq!(items.len(), 7);
    assert_eq!(kinds(&items), ["body", "face", "edge", "vertex", "plane", "axis", "feature"]);
    for (i, item) in items.iter().enumerate() {
        assert_eq!(item["kind"], item["type"], "item {i} of {r}");
    }
    // What each kind resolves to.
    assert!(items[1]["area_mm2"].is_number() && items[1]["planar"] == true && items[1]["surface"]["type"].is_string(), "{:?}", items[1]);
    assert!(items[2]["length_mm"].is_number() && items[2]["start"].is_array() && items[2]["end"].is_array(), "{:?}", items[2]);
    assert!(items[3]["position"].is_array(), "{:?}", items[3]);
    assert!(items[4]["normal"].is_array() && items[4]["point"].is_array(), "{:?}", items[4]);
    assert!(items[5]["origin"].is_array() && items[5]["direction"].is_array(), "{:?}", items[5]);
    assert!(items[6]["name"].is_string() && items[6]["feature_type"].is_string() && items[6]["suppressed"].is_boolean(), "{:?}", items[6]);

    // Without `detail` every item is listed by identity only.
    let r = run(&mut s, json!({"detail": false}));
    let bare = r["items"].as_array().cloned().unwrap_or_default();
    assert_eq!(bare.len(), 7);
    assert_eq!(bare[1]["kind"], "face");
    assert_eq!(bare[1]["body"], "Box1");
    assert!(bare.iter().all(|i| i.get("surface").is_none() && i.get("volume_mm3").is_none()), "{r}");

    // The sketch kinds: a rectangle, then its curves, points and profile.
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    s.execute("sketch.rectangle.center", &json!({"center": [0, 0], "corner": [10, 10]})).unwrap();
    let sk = s.execute("sketch.inspect", &json!({})).unwrap();
    let line = sk["curves"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["type"] == "line").map(|c| c["id"].as_str().unwrap_or_default().to_string()))
        .unwrap_or_default();
    let point = sk["points"][0]["id"].as_str().unwrap_or_default().to_string();
    let sketch = sk["id"].as_u64().unwrap_or_default();
    s.execute("sketch.finish", &json!({})).unwrap();
    select(
        &mut s,
        vec![
            json!({"type": "sketch_curve", "id": &line}),
            json!({"type": "sketch_point", "id": &point}),
            json!({"type": "sketch_midpoint", "id": &line}),
            json!({"type": "external_sketch_curve", "sketch": sketch, "id": &line}),
            json!({"type": "external_sketch_point", "sketch": sketch, "id": &point}),
            json!({"type": "sketch_constraint", "id": "not-there"}),
            json!({"type": "profile", "sketch": sketch, "index": 0}),
            json!({"type": "construction_point", "name": "not-there"}),
        ],
    );
    let r = run(&mut s, json!({"detail": true}));
    assert_eq!(r["count"], 8);
    let items = r["items"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        kinds(&items),
        [
            "sketch_curve",
            "sketch_point",
            "sketch_midpoint",
            "external_sketch_curve",
            "external_sketch_point",
            "sketch_constraint",
            "profile",
            "construction_point"
        ]
    );
    assert_eq!(items[0]["id"], line);
    assert_eq!(items[0]["sketch"], sketch, "{:?}", items[0]);
    assert_eq!(items[0]["entity"], "line", "{:?}", items[0]);
    assert!(items[0]["sketch_name"].is_string() && items[0]["construction"] == false, "{:?}", items[0]);
    assert!(items[1]["position"].is_array() && items[1]["entity"] == "point", "{:?}", items[1]);
    assert!(items[2]["position"].is_array() && items[2]["entity"] == "midpoint", "{:?}", items[2]);
    assert_eq!(items[3]["sketch"], sketch);
    assert!(items[3]["entity"] == "line" && items[4]["position"].is_array(), "{:?} {:?}", items[3], items[4]);
    // An item the design has no answer for is still listed, by identity only.
    assert_eq!(items[5]["id"], "not-there");
    assert!(items[5].get("sketch").is_none() && items[5].get("entity").is_none(), "{:?}", items[5]);
    assert!(items[6]["sketch_name"].is_string() && items[6]["area_mm2"].is_number(), "{:?}", items[6]);
    assert_eq!(items[7]["name"], "not-there");

    // An occurrence: what is selected is a placement, so the reply names the component it is in.
    let made = s.execute("component.create", &json!({"name": "Bracket"})).unwrap();
    let component = made["component"].as_u64().unwrap_or_default();
    let occurrence = made["occurrence"].as_u64().unwrap_or_default();
    assert!(component > 0 && occurrence > 0, "{made}");
    select(&mut s, vec![json!({"type": "occurrence", "path": [occurrence]})]);
    let r = run(&mut s, json!({}));
    let placed = &r["items"][0];
    assert_eq!(placed["kind"], "occurrence");
    assert_eq!(placed["path"], json!([occurrence]), "{placed:?}");
    assert_eq!(placed["component"]["id"], component, "{placed:?}");
    assert!(placed["name"].is_string() && placed["grounded"].is_boolean() && placed["bodies"].is_array(), "{placed:?}");

    // A misspelt key runs in Warn and says so; the reply still matches. Strict refuses it.
    let p = json!({"detil": false});
    let r = s.execute("selection.get", &p).unwrap();
    assert!(r["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{r}");
    assert!(conforms(&schema, &schema, &r), "{r} doesn't match {schema}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("selection.get", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`detil`"), "{error}");
    assert!(s.execute("selection.get", &json!({"detail": false})).is_ok());
}

/// A selection past the detail cap resolves the first `MAX_DETAIL` items in full, lists the rest
/// by identity only, and says so; without `detail` it says nothing.
#[test]
fn a_selection_past_the_cap_says_so() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box1"})).unwrap();
    // `select.set` keeps one of each item, so the selection is the same face picked at
    // different points: `MAX_DETAIL + 1` items, which is one past the cap.
    let items: Vec<Value> = (0..=MAX_DETAIL)
        .map(|i| json!({"type": "face", "body": "Box1", "point": [0.5 + (i % 91) as f64 / 10.0, 0.5 + (i / 91) as f64 / 10.0, 0.0]}))
        .collect();
    s.execute("select.set", &json!({"items": items})).unwrap_or_else(|e| panic!("{e}"));
    let r = s.execute("selection.get", &json!({})).unwrap();
    let schema = output_schema_for::<SelectionGetResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert_eq!(r["count"], MAX_DETAIL + 1);
    assert_eq!(r["detail_limit"], MAX_DETAIL);
    let items = r["items"].as_array().cloned().unwrap_or_default();
    assert_eq!(items.len(), MAX_DETAIL + 1);
    assert!(items[MAX_DETAIL - 1].get("surface").is_some(), "{:?}", items[MAX_DETAIL - 1]);
    assert!(items[MAX_DETAIL].get("surface").is_none(), "{:?}", items[MAX_DETAIL]);
    // The item past the cap is still listed as itself, with nothing resolved.
    assert_eq!(items[MAX_DETAIL]["kind"], "face");
    assert_eq!(items[MAX_DETAIL]["body"], "Box1");
    assert!(items[MAX_DETAIL].get("area_mm2").is_none(), "{:?}", items[MAX_DETAIL]);
    let r = s.execute("selection.get", &json!({"detail": false})).unwrap();
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    assert!(r.get("detail_limit").is_none(), "{r}");
    assert!(r["items"].as_array().is_some_and(|a| a.iter().all(|i| i.get("surface").is_none())), "{r}");
}
