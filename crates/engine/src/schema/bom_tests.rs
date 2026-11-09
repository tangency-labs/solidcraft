use serde_json::{Value, json};

use super::{Bom, BomParams, ComponentMetadataParams, ComponentMetadataResult, ComponentMetadataSetParams, ComponentMetadataSetResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// As in the command's own tests: a plate placed three times, and a hinge subassembly placed
/// twice that holds one pin. Here the hinge has a part number too, so it can keep its pin.
fn assembly(s: &mut Session) {
    run(s, "component.create", json!({"name": "Plate"}));
    run(s, "solid.box", json!({"length": 40, "width": 20, "height": 3}));
    run(s, "component.activate", json!({"component": 0}));
    run(s, "component.create", json!({"name": "Hinge", "activate": false}));
    run(s, "component.create", json!({"name": "Pin", "parent": "Hinge"}));
    run(s, "solid.cylinder", json!({"diameter": 4, "height": 30}));
    run(s, "component.activate", json!({"component": 0}));
    for x in [50, 100] {
        run(s, "occurrence.copy", json!({"component": "Plate", "translate": [x, 0, 0]}));
    }
    run(s, "occurrence.copy", json!({"component": "Hinge", "translate": [0, 50, 0]}));
    run(s, "component.metadata.set", json!({"component": "Plate", "part_number": "PL-100", "description": "Plate, 3 mm", "revision": "B"}));
    run(s, "component.metadata.set", json!({"component": "Pin", "part_number": "PN-040", "properties": {"finish": "zinc"}}));
    run(s, "component.metadata.set", json!({"component": "Hinge", "part_number": "HG-1"}));
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<BomParams>(), &["component", "format", "group_by", "group_children", "levels", "path"]);
    assert_keys(input_schema_for::<ComponentMetadataParams>(), &["component"]);
    assert_keys(
        input_schema_for::<ComponentMetadataSetParams>(),
        &["bom_type", "component", "description", "part_number", "properties", "revision", "values"],
    );
}

#[test]
fn strict_keeps_todays_errors() {
    let file = std::env::temp_dir().join(format!("solidcraft-bom-schema-{}.csv", std::process::id()));
    let file = file.to_string_lossy();
    assert_strict_keeps_todays_errors(&[
        ("assembly.bom", json!({"component": true})),
        ("assembly.bom", json!({"component": "Nope"})),
        ("assembly.bom", json!({"levels": "deep"})),
        ("assembly.bom", json!({"levels": 1})),
        ("assembly.bom", json!({"group_by": "size"})),
        ("assembly.bom", json!({"group_by": ["size"]})),
        ("assembly.bom", json!({"group_by": ["part_number", "definition"]})),
        ("assembly.bom", json!({"group_by": [5]})),
        ("assembly.bom", json!({"group_by": 5})),
        ("assembly.bom", json!({"group_children": "yes"})),
        ("assembly.bom", json!({"path": file.replace(".csv", ".txt")})),
        ("assembly.bom", json!({"path": file, "format": "xml"})),
        ("component.metadata", json!({})),
        ("component.metadata", json!({"component": null})),
        ("component.metadata", json!({"component": true})),
        ("component.metadata", json!({"component": [1]})),
        ("component.metadata", json!({"component": "Nope"})),
        ("component.metadata.set", json!({})),
        ("component.metadata.set", json!({"component": null, "part_number": "A"})),
        ("component.metadata.set", json!({"component": {"id": 1}, "part_number": "A"})),
        ("component.metadata.set", json!({"component": "Nope", "part_number": "A"})),
        ("component.metadata.set", json!({"component": 0, "part_number": "A"})),
        ("component.metadata.set", json!({"component": "root", "part_number": "A"})),
        ("component.metadata.set", json!({"component": 0, "values": 5})),
    ]);
    // The cases that need a component to be there: a part with properties to set badly.
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let mut off = Session { param_check: ParamCheck::Off, ..Session::default() };
    for session in [&mut s, &mut off] {
        run(session, "component.create", json!({"name": "Plate"}));
    }
    for p in [
        json!({"component": "Plate"}),
        json!({"component": "Plate", "values": null}),
        json!({"component": "Plate", "values": [1]}),
        json!({"component": "Plate", "values": "x"}),
        json!({"component": "Plate", "properties": null}),
        json!({"component": "Plate", "properties": ["a"]}),
        json!({"component": "Plate", "values": (0..257).map(|i| (format!("org.k{i}"), json!({}))).collect::<serde_json::Map<_, _>>()}),
        json!({"component": "Plate", "properties": (0..257).map(|i| (format!("p{i}"), json!("x"))).collect::<serde_json::Map<_, _>>()}),
        json!({"component": "Plate", "part_number": 5}),
        json!({"component": "Plate", "part_number": ["A"]}),
        json!({"component": "Plate", "bom_type": "kit"}),
        json!({"component": "Plate", "bom_type": 1}),
        json!({"component": "Plate", "values": {"nope": "x"}}),
        json!({"component": "Plate", "values": {"com.example.tool": "x"}}),
        json!({"component": "Plate", "properties": {"": "x"}}),
        json!({"component": "Plate", "properties": {"a": ["x"]}}),
        json!({"component": "Plate", "properties": {"a": {"b": 1}}}),
        json!({"component": "Plate", "description": "bad\u{7}"}),
        json!({"component": "Plate", "properties": {"a": "bad\u{7}"}}),
    ] {
        let (a, b) = (off.execute("component.metadata.set", &p), s.execute("component.metadata.set", &p));
        assert!(a.is_err(), "{p} should fail today");
        assert_eq!(a.map_err(|e| e.to_string()), b.map_err(|e| e.to_string()), "{p}");
    }
}

/// #11075: a bill of materials replies as its output schema says, in Warn without warnings for
/// good calls and with them for a misspelt key: flat and top, grouped by definition, with the
/// hinge keeping its pin as children, and written to a JSON and a CSV file.
#[test]
fn a_bill_of_materials_replies_with_its_schema() {
    let schema = output_schema_for::<Bom>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    assembly(&mut s);
    let r = run(&mut s, "assembly.bom", json!({}));
    matches(&r, false);
    assert!(r.get("path").is_none() && r["rows"].as_array().is_some_and(|rows| rows.iter().all(|row| row.get("children").is_none())), "{r}");
    matches(&run(&mut s, "assembly.bom", json!({"component": 0, "levels": "top", "group_by": "definition"})), false);
    let r = run(&mut s, "assembly.bom", json!({"levels": "flat", "group_by": ["part_number"], "group_children": true}));
    matches(&r, false);
    let hinge = r["rows"].as_array().and_then(|rows| rows.iter().find(|row| row["part_number"] == "HG-1")).cloned().unwrap_or_default();
    assert_eq!((hinge["children"][0]["part_number"].as_str(), hinge["children"][0]["item"].is_string()), (Some("PN-040"), true), "{r}");
    let dir = std::env::temp_dir();
    for (name, format) in [("json", Value::Null), ("csv", json!("CSV"))] {
        let path = dir.join(format!("solidcraft-bom-schema-{}.{name}", std::process::id())).to_string_lossy().into_owned();
        let r = run(&mut s, "assembly.bom", json!({"path": path, "format": format}));
        matches(&r, false);
        assert_eq!(r["format"], name, "{r}");
        let _ = std::fs::remove_file(&path);
    }
    matches(&run(&mut s, "assembly.bom", json!({"level": "top"})), true);
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value, warned: bool) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert_eq!(reply.get("param_warnings").is_some(), warned, "{id}: {reply}");
}

/// #12609: the types read each key as the handlers do, with their messages, and the check
/// accepts every form a handler takes today.
#[test]
fn the_part_property_types_read_keys_as_the_handlers_do() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    let none = Vec::<String>::new();
    for id in ["component.metadata", "component.metadata.set"] {
        let p = |component: Value| {
            if id == "component.metadata" { json!({"component": component}) } else { json!({"component": component, "revision": "A"}) }
        };
        assert_eq!(issues(id, p(json!(3))), none, "{id}");
        assert_eq!(issues(id, p(json!("Plate"))), none, "{id}");
        assert_eq!(issues(id, p(json!(0))), none, "{id}");
        assert_eq!(issues(id, p(json!("root"))), none, "{id}");
        for bad in [json!(null), json!(true), json!([1]), json!({"a": 1})] {
            assert_eq!(issues(id, p(bad.clone())), ["`component` must be a component id or name"], "{id} {bad}");
        }
        assert_eq!(issues(id, json!({"component": 1, "compnent": 1})).len(), 1, "{id}");
    }
    let set = |p: Value| issues("component.metadata.set", p);
    // The keys that take text hand any value to the handler, which names what it refuses.
    for k in ["part_number", "description", "revision", "bom_type"] {
        for v in [json!("x"), json!(""), json!(null), json!(5), json!(["x"])] {
            assert_eq!(set(json!({"component": 1, k: v})), none, "{k}");
        }
    }
    assert_eq!(set(json!({"component": 1, "values": {"com.example.tool": {"a": 1}, "author": 5}})), none);
    assert_eq!(set(json!({"component": 1, "properties": {"a": "x", "b": 2, "c": true, "d": null, "e": ["x"]}})), none);
    // For the design the handler refuses before it reads anything else, so nothing else is read.
    for root in [json!(0), json!("0"), json!("root"), json!("ROOT")] {
        assert_eq!(set(json!({"component": root, "values": 5, "properties": null})), none, "{root}");
    }
    assert_eq!(set(json!({"component": 1, "values": null})), ["`values` must be an object: {key: value, …}"]);
    assert_eq!(set(json!({"component": 1, "values": [1]})), ["`values` must be an object: {key: value, …}"]);
    assert_eq!(set(json!({"component": 1, "properties": "a"})), ["`properties` must be an object: {name: text, …}"]);
    let many = |prefix: &str| (0..257).map(|i| (format!("{prefix}{i}"), json!("x"))).collect::<serde_json::Map<_, _>>();
    assert_eq!(set(json!({"component": 1, "values": many("org.k")})), ["`values` can hold at most 256 keys"]);
    assert_eq!(set(json!({"component": 1, "properties": many("p")})), ["`properties` can hold at most 256 names"]);
    let mut full = many("p");
    full.remove("p256");
    assert_eq!(set(json!({"component": 1, "properties": full, "values": {}})), none);
}

/// The schemas say what the handlers need: `component` is required though the types leave it to
/// the handlers' message, and the limits and shapes are those of the handlers.
#[test]
fn the_part_property_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<ComponentMetadataParams>(),
        &[(json!({"component": "Plate"}), true), (json!({"component": 0}), true), (json!({}), false), (json!({"component": true}), false)],
    );
    takes(
        input_schema_for::<ComponentMetadataSetParams>(),
        &[
            (json!({"component": "Plate", "part_number": "P-1", "description": null, "revision": "", "bom_type": "phantom"}), true),
            (
                json!({"component": 2, "values": {"com.example.tool": {"a": 1}}, "properties": {"finish": "zinc", "n": 3, "b": true, "gone": null}}),
                true,
            ),
            (json!({"part_number": "P-1"}), false),
            (json!({"component": "Plate", "part_number": 5}), false),
            (json!({"component": "Plate", "values": []}), false),
        ],
    );
}

/// Read and change part properties of a component and of the design, in Warn: each real reply
/// matches its output schema and carries no warning; a misspelt key's warning still matches.
#[test]
fn part_property_replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    assembly(&mut s);
    let read = |s: &mut Session, p: Value| {
        let r = run(s, "component.metadata", p);
        matches(output_schema_for::<ComponentMetadataResult>(), "component.metadata", &r, false);
        r
    };
    let change = |s: &mut Session, p: Value| {
        let r = run(s, "component.metadata.set", p);
        matches(output_schema_for::<ComponentMetadataSetResult>(), "component.metadata.set", &r, false);
        r
    };
    let plate = read(&mut s, json!({"component": "Plate"}));
    assert_eq!(
        (plate["part_number"].as_str(), plate["revision"].as_str(), plate["bom_type"].as_str()),
        (Some("PL-100"), Some("B"), Some("normal")),
        "{plate}"
    );
    let pin = read(&mut s, json!({"component": "Pin"}));
    assert_eq!((pin["properties"]["finish"].as_str(), pin["revision"].is_null()), (Some("zinc"), true), "{pin}");
    let root = read(&mut s, json!({"component": 0}));
    assert!(root.get("bom_type").is_none() && root["properties"] == json!({}), "{root}");
    read(&mut s, json!({"component": "root"}));
    let by_id = read(&mut s, json!({"component": plate["component"]}));
    assert_eq!(by_id["name"], "Plate");

    let r = change(
        &mut s,
        json!({"component": "Plate", "revision": "C", "bom_type": "purchased", "properties": {"material": "1.4301", "n": 3, "ok": true}}),
    );
    assert_eq!(r["changed"], json!(["revision", "bom_type", "properties.material", "properties.n", "properties.ok"]), "{r}");
    let r = change(
        &mut s,
        json!({"component": "Plate", "values": {"com.example.tool": {"a": 1}, "description": "Plate"}, "part_number": null, "properties": {"n": ""}}),
    );
    assert_eq!(r["changed"], json!(["com.example.tool", "description", "part_number", "properties.n"]), "{r}");
    assert_eq!(r["properties"], json!({"material": "1.4301", "ok": "true"}), "{r}");
    let r = change(&mut s, json!({"component": "Plate", "revision": "C"}));
    assert_eq!(r["changed"], json!([]), "{r}");
    assert_eq!(read(&mut s, json!({"component": "Plate"}))["metadata"]["com.example.tool"], json!({"a": 1}));

    for (id, p) in [
        ("component.metadata", json!({"component": "Plate", "compnent": 1})),
        ("component.metadata.set", json!({"component": "Plate", "revision": "D", "revison": "E"})),
    ] {
        let warned = run(&mut s, id, p);
        assert!(warned["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{warned}");
        if id == "component.metadata" {
            matches(output_schema_for::<ComponentMetadataResult>(), id, &warned, true);
        } else {
            matches(output_schema_for::<ComponentMetadataSetResult>(), id, &warned, true);
        }
    }
}
