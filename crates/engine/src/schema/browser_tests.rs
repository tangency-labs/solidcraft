use serde_json::{Value, json};

use super::{
    BodyRenameParams, BodyRenamed, BrowserFoldParams, BrowserFoldResult, BrowserGroupMade, BrowserGroupParams, BrowserGroupRenamed,
    BrowserMoveParams, BrowserMoved, BrowserOrderParams, BrowserOrdered, BrowserRenameGroupParams, BrowserUngroupParams, BrowserUngrouped,
    BrowserVisibilityParams, BrowserVisibilityResult, DocumentUnitsParams, DocumentUnitsResult, FindParams, FindResult, RepairParams, RepairResult,
    VisibilityState,
};
use crate::Session;
use crate::browser_find::MAX_PATTERN_CHARS;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms, sketch_rectangle};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<RepairParams>(), &["body", "body_name", "body_names", "gap", "name", "participants", "patch_holes"]);
}

#[test]
fn replies_match_the_output_schema() {
    let schema = output_schema_for::<RepairResult>();
    let schema = schema.as_value();
    let required: Vec<&str> = schema.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
    assert!(!required.contains(&"param_warnings"));
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"})).unwrap();
    s.execute("surface.unstitch", &json!({"body": "Box", "faces": [[5, 5, 10]]})).unwrap();
    s.param_check = ParamCheck::Warn;
    let r = s.execute("body.repair", &json!({"body": "Box", "gap": "0.1 mm", "patch_holes": true, "name": "Mend"})).unwrap();
    assert!(r.get("param_warnings").is_none(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    let r = s.execute("body.repair", &json!({"body": "Box", "colour": "red"}));
    assert!(r.is_err(), "nothing left to repair");
}

#[test]
fn units_schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<DocumentUnitsParams>(), &["units"]);
}

#[test]
fn units_strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[("document.units", json!({})), ("document.units", json!({"units": "furlong"}))]);
}

/// A change that keeps a parameter's value with the old unit, one that keeps none, and the same
/// units again, in Warn: each reply matches the output schema and carries no warning.
#[test]
fn units_replies_match_the_output_schema() {
    let schema = output_schema_for::<DocumentUnitsResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |p: Value| s.execute("document.units", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let mut check = |p: Value| {
        let reply = run(p.clone());
        assert!(reply.get("param_warnings").is_none() && conforms(schema, schema, &reply), "{p}: {reply} doesn't match {schema}");
        reply
    };
    check(json!({"units": "cm"}));
    assert_eq!(check(json!({"units": "cm"}))["kept"], json!([]));
    check(json!({"units": "in"}));
}

/// A unit-less parameter a length reads is kept with the old unit, and the reply names it.
#[test]
fn the_reply_names_the_parameters_kept_in_the_old_unit() {
    let mut s = Session::default();
    s.execute("parameters.add", &json!({"name": "w", "expression": "10", "unit": ""})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("solid.box", &json!({"length": "w", "width": 10, "height": 10})).unwrap_or_else(|e| panic!("{e}"));
    let reply = s.execute("document.units", &json!({"units": "in"})).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<DocumentUnitsResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply}");
    assert_eq!(reply["kept"], json!(["w"]), "{reply}");
}

#[test]
fn find_schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<FindParams>(), &["kinds", "text"]);
}

#[test]
fn find_strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("browser.find", json!({})),
        ("browser.find", json!({"text": "   "})),
        ("browser.find", json!({"text": "   ", "kinds": []})),
        ("browser.find", json!({"text": null, "kinds": null})),
        ("browser.find", json!({"text": 5})),
        ("browser.find", json!({"text": ["bolt"]})),
        ("browser.find", json!({"text": "a".repeat(MAX_PATTERN_CHARS + 1)})),
        ("browser.find", json!({"kinds": "body"})),
        ("browser.find", json!({"kinds": ["bodies"]})),
        ("browser.find", json!({"kinds": [1]})),
        ("browser.find", json!({"kinds": [null]})),
        ("browser.find", json!({"kinds": ["body", "body", "body", "body", "body", "body"]})),
    ]);
}

#[test]
fn find_replies_match_the_output_schema() {
    let schema = output_schema_for::<FindResult>();
    let schema = schema.as_value();
    let mut s = Session::default();
    sketch_rectangle(&mut s);
    s.execute("solid.box", &json!({"length": 5, "width": 5, "height": 5})).unwrap_or_else(|e| panic!("{e}"));
    s.param_check = ParamCheck::Warn;
    let forms = [
        json!({"text": "sketch"}),
        json!({"kinds": ["body"]}),
        json!({"text": "*1", "kinds": ["sketch", "body", "component", "construction", "joint"]}),
        json!({"text": "  box  ", "kinds": []}),
        json!({"text": "nothing is called this"}),
        json!({"text": "a".repeat(MAX_PATTERN_CHARS), "kinds": null}),
    ];
    for p in forms {
        let r = s.execute("browser.find", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(r.get("param_warnings").is_none(), "{p}: {r}");
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    }
    // A reply carrying warnings still matches.
    let r = s.execute("browser.find", &json!({"text": "box", "kind": "body"})).unwrap_or_else(|e| panic!("{e}"));
    assert!(r.get("param_warnings").is_some(), "{r}");
    assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
    // The check itself catches a wrong type, a wrong item type and a missing key.
    let ok = json!({"found": [{"kind": "body", "name": "Box1", "key": "Box1", "path": [], "item": {"type": "body", "name": "Box1"}}], "total": 1, "incomplete": false});
    assert!(conforms(schema, schema, &ok));
    for (k, v) in [("total", json!("1")), ("found", json!([1])), ("incomplete", json!(0))] {
        let mut bad = ok.clone();
        bad[k] = v;
        assert!(!conforms(schema, schema, &bad), "{bad}");
    }
    let mut bad = ok.clone();
    bad["found"][0]["item"] = Value::from("Box1");
    assert!(!conforms(schema, schema, &bad), "{bad}");
    bad = ok.clone();
    bad.as_object_mut().map(|o| o.remove("total"));
    assert!(!conforms(schema, schema, &bad), "{bad}");
}

#[test]
fn browser_schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<BodyRenameParams>(), &["body", "name"]);
    assert_keys(input_schema_for::<BrowserGroupParams>(), &["component", "folder", "items", "name", "parent"]);
    assert_keys(input_schema_for::<BrowserUngroupParams>(), &["group"]);
    assert_keys(input_schema_for::<BrowserRenameGroupParams>(), &["group", "name"]);
    assert_keys(input_schema_for::<BrowserMoveParams>(), &["before", "component", "folder", "group", "items"]);
    assert_keys(input_schema_for::<BrowserOrderParams>(), &["component", "folder", "order"]);
    assert_keys(
        input_schema_for::<BrowserVisibilityParams>(),
        &["features", "folder", "isolate", "items", "joints", "path", "paths", "tree_folders", "visible"],
    );
    assert_keys(input_schema_for::<BrowserFoldParams>(), &["component", "path"]);
}

/// Each call has one problem the handler already refuses: strict gives its message. (A missing
/// key is left to the handler, so the message is the handler's too.)
#[test]
fn browser_strict_keeps_todays_errors() {
    let long = "n".repeat(129);
    let deep: Vec<u64> = (1..=65).collect();
    assert_strict_keeps_todays_errors(&[
        ("body.rename", json!({})),
        ("body.rename", json!({"body": 5, "name": "X"})),
        ("body.rename", json!({"body": null, "name": "X"})),
        ("browser.group", json!({})),
        ("browser.group", json!({"folder": "components"})),
        ("browser.group", json!({"folder": null})),
        ("browser.group", json!({"folder": "bodies", "parent": "x"})),
        ("browser.group", json!({"folder": "bodies", "name": long})),
        ("browser.group", json!({"folder": "bodies", "items": [1]})),
        ("browser.group", json!({"folder": "bodies", "items": "A"})),
        ("browser.group", json!({"folder": "bodies", "items": null})),
        ("browser.group", json!({"folder": "bodies", "items": [""]})),
        ("browser.group", json!({"folder": "bodies", "items": [true]})),
        ("browser.move", json!({"folder": "bodies", "items": vec!["A"; 10_001]})),
        ("browser.move", json!({"folder": "bodies", "items": ["k".repeat(257)]})),
        ("browser.order", json!({"folder": "bodies", "order": vec!["A"; 10_001]})),
        ("browser.ungroup", json!({})),
        ("browser.ungroup", json!({"group": "g"})),
        ("browser.ungroup", json!({"group": null})),
        ("browser.ungroup", json!({"group": 99})),
        ("browser.rename_group", json!({})),
        ("browser.rename_group", json!({"group": -1, "name": "x"})),
        ("browser.move", json!({})),
        ("browser.move", json!({"folder": "bodies"})),
        ("browser.move", json!({"folder": "bodies", "items": []})),
        ("browser.move", json!({"folder": "bodies", "items": [true]})),
        ("browser.move", json!({"folder": "bodies", "items": [""]})),
        ("browser.move", json!({"folder": "bodies", "items": "A"})),
        ("browser.move", json!({"folder": "bodies", "items": ["A"], "group": "g"})),
        ("browser.move", json!({"folder": "bodies", "items": ["A"], "before": true})),
        ("browser.move", json!({"folder": "origin", "items": ["A"]})),
        ("browser.order", json!({})),
        ("browser.order", json!({"folder": "origin"})),
        ("browser.order", json!({"folder": "bodies", "order": null})),
        ("browser.order", json!({"folder": "bodies", "order": [true]})),
        ("browser.order", json!({"folder": "bodies", "order": ["A", ""]})),
        ("browser.visibility", json!({})),
        ("browser.visibility", json!({"visible": "yes"})),
        ("browser.visibility", json!({"visible": true})),
        ("browser.visibility", json!({"visible": true, "tree_folders": []})),
        ("browser.visibility", json!({"visible": true, "tree_folders": ["bodies"]})),
        ("browser.visibility", json!({"visible": true, "tree_folders": [3]})),
        ("browser.visibility", json!({"visible": true, "paths": []})),
        ("browser.visibility", json!({"visible": true, "paths": [[]]})),
        ("browser.visibility", json!({"visible": true, "paths": [["a"]]})),
        ("browser.visibility", json!({"visible": true, "paths": [deep]})),
        ("browser.visibility", json!({"visible": true, "joints": []})),
        ("browser.visibility", json!({"visible": true, "joints": [true]})),
        ("browser.visibility", json!({"visible": true, "folder": "nowhere"})),
        ("browser.visibility", json!({"visible": true, "items": "A"})),
        ("browser.visibility", json!({"visible": true, "items": null})),
        ("browser.visibility", json!({"visible": true, "items": [""]})),
        ("browser.visibility", json!({"visible": true, "items": []})),
        ("browser.visibility", json!({"visible": true, "features": [true]})),
        ("browser.expand_all", json!({"path": []})),
        ("browser.expand_all", json!({"path": null})),
        ("browser.expand_all", json!({"path": "o:1"})),
        ("browser.expand_all", json!({"path": [1, "a"]})),
        ("browser.expand_all", json!({"component": "root"})),
        ("browser.collapse_all", json!({"component": null})),
        ("browser.collapse_all", json!({"component": -1})),
        ("browser.collapse_all", json!({"path": deep})),
    ]);
}

/// The same, for the keys that need a body or a group to get as far as their message.
#[test]
fn browser_strict_keeps_todays_errors_for_names() {
    let long = "n".repeat(129);
    let cases = [
        ("body.rename", json!({"body": "Box", "name": ""})),
        ("body.rename", json!({"body": "Box", "name": "   "})),
        ("body.rename", json!({"body": "Box", "name": 5})),
        ("body.rename", json!({"body": "Box", "name": null})),
        ("body.rename", json!({"body": "Box", "name": long})),
        ("body.rename", json!({"body": "Box"})),
        ("browser.rename_group", json!({"group": "@g", "name": ""})),
        ("browser.rename_group", json!({"group": "@g", "name": 5})),
        ("browser.rename_group", json!({"group": "@g", "name": long})),
        ("browser.rename_group", json!({"group": "@g"})),
        ("browser.group", json!({"parent": "@g", "folder": "sketches"})),
        ("browser.group", json!({"parent": "@g", "folder": null})),
        ("browser.group", json!({"folder": "bodies", "parent": 77})),
        ("browser.move", json!({"folder": "bodies", "items": ["Box"], "before": 77})),
    ];
    for (id, p) in cases {
        let run = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"})).unwrap();
            let g = s.execute("browser.group", &json!({"folder": "bodies", "name": "G"})).unwrap()["group"].clone();
            let p: Value = serde_json::from_str(&p.to_string().replace("\"@g\"", &g.to_string())).unwrap();
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = run(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(run(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// The contracts say what the handlers need: the keys that can't be left out are required, an
/// order may be left out, and a folder is one the handler takes.
#[test]
fn the_browser_schemas_take_what_the_handlers_read() {
    let required = |schema: schemars::Schema| {
        let v = schema.as_value();
        let mut r: Vec<String> =
            v.get("required").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        r.sort();
        r
    };
    assert_eq!(required(input_schema_for::<BodyRenameParams>()), ["body", "name"]);
    assert_eq!(required(input_schema_for::<BrowserGroupParams>()), Vec::<String>::new());
    assert_eq!(required(input_schema_for::<BrowserUngroupParams>()), ["group"]);
    assert_eq!(required(input_schema_for::<BrowserRenameGroupParams>()), ["group", "name"]);
    assert_eq!(required(input_schema_for::<BrowserMoveParams>()), ["folder", "items"]);
    assert_eq!(required(input_schema_for::<BrowserOrderParams>()), ["folder"]);
    assert_eq!(required(input_schema_for::<BrowserVisibilityParams>()), ["visible"]);
    assert_eq!(required(input_schema_for::<BrowserFoldParams>()), Vec::<String>::new());
    let mv = input_schema_for::<BrowserMoveParams>();
    let mv = mv.as_value();
    for (p, takes) in [
        (json!({"folder": "bodies", "items": ["A", 3], "group": 3, "before": "B"}), true),
        (json!({"folder": "bodies", "items": []}), false),
        (json!({"folder": "origin", "items": ["A"]}), false),
        (json!({"items": ["A"]}), false),
    ] {
        assert_eq!(conforms(mv, mv, &p), takes, "{p}");
    }
    let order = input_schema_for::<BrowserOrderParams>();
    let order = order.as_value();
    assert!(conforms(order, order, &json!({"folder": "components", "order": []})));
}

/// The typed visibility has the keys the saved one has, with every field set.
#[test]
fn the_visibility_reply_type_matches_the_design_s() {
    let v = solidcraft_doc::Visibility {
        hidden_bodies: vec!["A".into()],
        hidden_occurrences: vec![vec![1, 2]],
        hidden_origin: vec!["X".into()],
        hidden_sketches: vec![4],
        shown_sketches: vec![5],
        hidden_joints: vec![6],
        origin_hidden: Some(true),
        hidden_profiles: vec![7],
        shown_dimensions: vec![8],
        locked_bodies: vec!["B".into()],
        hidden_folders: vec!["c0/bodies".into()],
    };
    assert_eq!(serde_json::to_value(VisibilityState::from(&v)).unwrap(), serde_json::to_value(&v).unwrap());
    let empty = solidcraft_doc::Visibility::default();
    assert_eq!(serde_json::to_value(VisibilityState::from(&empty)).unwrap(), serde_json::to_value(&empty).unwrap());
}

/// #12276: every command replies as its output schema says, in Warn without warnings, in each
/// form it takes; a misspelt key's warning still matches.
#[test]
fn browser_commands_reply_with_their_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    // A placed component, for the placement forms.
    let part = s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap_or_else(|e| panic!("{e}"));
    s.execute("component.activate", &json!({"component": 0})).unwrap_or_else(|e| panic!("{e}"));
    let occ = part["occurrence"].clone();
    let mut run = |id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        assert!(conforms(schema, schema, &r), "{id} {p}: {r} doesn't match {schema}");
        r
    };
    for (i, name) in ["A", "B", "C"].into_iter().enumerate() {
        run(
            "solid.box",
            json!({"length": 10, "width": 10, "height": 10, "corner": [i as f64 * 20.0, 0, 0], "body_name": name}),
            output_schema_for::<crate::schema::features::FeatureResult>(),
        );
    }
    let made = run("browser.group", json!({"folder": "bodies", "items": ["A"], "name": " Parts "}), output_schema_for::<BrowserGroupMade>());
    assert_eq!(made["parent"], Value::Null, "{made}");
    let outer = made["group"].clone();
    let inner = run("browser.group", json!({"parent": outer, "items": ["B"], "component": 0}), output_schema_for::<BrowserGroupMade>());
    assert_eq!(inner["parent"], outer, "{inner}");
    let renamed = run("browser.rename_group", json!({"group": inner["group"], "name": " Inner "}), output_schema_for::<BrowserGroupRenamed>());
    assert_eq!(renamed["name"], "Inner", "{renamed}");
    let moved = run(
        "browser.move",
        json!({"folder": "bodies", "items": ["C", inner["group"]], "group": outer, "before": "A"}),
        output_schema_for::<BrowserMoved>(),
    );
    assert_eq!(moved["items"], json!(["C", inner["group"]]), "{moved}");
    let top = run("browser.move", json!({"folder": "bodies", "items": ["C"], "group": null}), output_schema_for::<BrowserMoved>());
    assert_eq!(top["group"], Value::Null, "{top}");
    run("browser.order", json!({"folder": "bodies", "order": ["C", outer]}), output_schema_for::<BrowserOrdered>());
    let cleared = run("browser.order", json!({"folder": "bodies"}), output_schema_for::<BrowserOrdered>());
    assert_eq!(cleared, json!({}), "{cleared}");
    run("browser.ungroup", json!({"group": inner["group"]}), output_schema_for::<BrowserUngrouped>());

    let visibility = output_schema_for::<BrowserVisibilityResult>;
    for p in [
        json!({"visible": false, "items": ["A", "xy"]}),
        json!({"visible": true, "items": ["A"]}),
        json!({"visible": false, "folder": "bodies"}),
        json!({"visible": true, "folder": "all"}),
        json!({"visible": false, "features": ["Box1"]}),
        json!({"visible": false, "tree_folders": ["c0/bodies", format!("c0/g{outer}")]}),
        json!({"visible": false, "paths": [[occ]]}),
        json!({"visible": true, "paths": [[occ]], "isolate": true}),
        json!({"visible": true, "items": ["B"], "isolate": true}),
    ] {
        let r = run("browser.visibility", p.clone(), visibility());
        assert!(r["visibility"].is_object(), "{p}: {r}");
        assert_eq!(r.get("isolated").is_some(), p.get("isolate").is_some(), "{p}: {r}");
        assert_eq!(r.get("changed").is_some(), p.get("isolate").is_none(), "{p}: {r}");
    }
    let fold = output_schema_for::<BrowserFoldResult>;
    for (id, p, open) in [
        ("browser.expand_all", json!({}), true),
        ("browser.expand_all", json!({"path": [occ]}), true),
        ("browser.collapse_all", json!({"component": 0}), false),
        ("browser.collapse_all", json!({"path": [occ]}), false),
    ] {
        assert_eq!(run(id, p, fold())["open"], open);
    }
    let renamed = run("body.rename", json!({"body": "A", "name": " Alpha "}), output_schema_for::<BodyRenamed>());
    assert_eq!(renamed, json!({"body": "A", "name": "Alpha"}), "{renamed}");
    let same = run("body.rename", json!({"body": "Alpha", "name": "Alpha"}), output_schema_for::<BodyRenamed>());
    assert_eq!(same, json!({"body": "Alpha", "name": "Alpha"}), "{same}");

    // A misspelt key still runs in warn, and says so; strict refuses it.
    let p = json!({"visible": true, "folder": "all", "itms": []});
    let r = s.execute("browser.visibility", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let schema = output_schema_for::<BrowserVisibilityResult>();
    assert!(r["param_warnings"].to_string().contains("itms") && conforms(schema.as_value(), schema.as_value(), &r), "{r}");
    s.param_check = ParamCheck::Strict;
    let error = s.execute("browser.visibility", &p).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`itms`"), "{error}");
}
