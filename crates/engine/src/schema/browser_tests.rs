use serde_json::{Value, json};

use super::{
    BodyRenameParams, BodyRenamed, BrowserFoldParams, BrowserFoldResult, BrowserGroupMade, BrowserGroupParams, BrowserGroupRenamed,
    BrowserMoveParams, BrowserMoved, BrowserOrderParams, BrowserOrdered, BrowserRenameGroupParams, BrowserUngroupParams, BrowserUngrouped,
    BrowserVisibilityParams, BrowserVisibilityResult, DocumentUnitsParams, DocumentUnitsResult, RedefineSketchParams, RedefineSketchResult,
    RepairParams, RepairResult, ViewApplyParams, ViewDeleteParams, ViewDeleted, ViewListParams, ViewListResult, ViewRenameParams, ViewRenamed,
    ViewSaveParams, ViewSaved, VisibilityState,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::view_navigation::CameraReply;
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

// ---- Named views and Redefine Sketch (#12499) ----

const CAMERA: fn() -> Value = || json!({"target": [1, 2, 3], "yaw": 0.5, "pitch": -0.6, "distance": 150});

#[test]
fn view_schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ViewSaveParams>(), &["camera", "name"]);
    assert_keys(input_schema_for::<ViewRenameParams>(), &["name", "view"]);
    assert_keys(input_schema_for::<ViewDeleteParams>(), &["view"]);
    assert_keys(input_schema_for::<ViewListParams>(), &[]);
    assert_keys(input_schema_for::<ViewApplyParams>(), &["camera", "view"]);
    assert_keys(input_schema_for::<RedefineSketchParams>(), &["author_path", "frame", "offset", "plane", "sketch"]);
}

/// Each call has one problem the handler already refuses: strict gives its message. The session
/// has a sketch (`1`) and no named view, so a view that must exist is looked for in the second
/// test.
#[test]
fn view_strict_keeps_todays_errors() {
    let long = "n".repeat(129);
    let deep: Vec<u64> = (1..=65).collect();
    assert_strict_keeps_todays_errors(&[
        ("view.save", json!({})),
        ("view.save", json!({"name": "A"})),
        ("view.save", json!({"camera": null})),
        ("view.save", json!({"camera": "front"})),
        ("view.save", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0}})),
        ("view.save", json!({"camera": {"target": [0, 0], "yaw": 0, "pitch": 0, "distance": 5}})),
        ("view.save", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0}})),
        ("view.save", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": -3}})),
        ("view.save", json!({"camera": CAMERA(), "name": long})),
        ("view.rename", json!({})),
        ("view.rename", json!({"name": "B"})),
        ("view.rename", json!({"view": 5, "name": "B"})),
        ("view.rename", json!({"view": null, "name": "B"})),
        ("view.rename", json!({"view": "Nope", "name": "B"})),
        ("view.delete", json!({})),
        ("view.delete", json!({"view": 5})),
        ("view.delete", json!({"view": null})),
        ("view.delete", json!({"view": "Nope"})),
        ("view.apply", json!({})),
        ("view.apply", json!({"view": 5})),
        ("view.apply", json!({"view": null})),
        ("view.apply", json!({"view": "Nope"})),
        ("view.apply", json!({"view": "top", "camera": CAMERA()})),
        ("view.apply", json!({"view": "top", "camera": null})),
        ("view.apply", json!({"camera": null})),
        ("view.apply", json!({"camera": "front"})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "up": "y"}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "up": null}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "roll": "x"}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "roll": null}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 1.6, "distance": 80}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": -1.6, "distance": 80}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 0}})),
        ("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": 0, "distance": 80, "fov": -0.5}})),
        ("sketch.redefine", json!({})),
        ("sketch.redefine", json!({"plane": "XZ"})),
        ("sketch.redefine", json!({"sketch": true, "plane": "XZ"})),
        ("sketch.redefine", json!({"sketch": null, "plane": "XZ"})),
        ("sketch.redefine", json!({"sketch": [1], "plane": "XZ"})),
        ("sketch.redefine", json!({"sketch": 999, "plane": "XZ"})),
        ("sketch.redefine", json!({"sketch": 1})),
        ("sketch.redefine", json!({"sketch": 1, "plane": null})),
        ("sketch.redefine", json!({"sketch": 1, "plane": true})),
        ("sketch.redefine", json!({"sketch": 1, "plane": 1.5})),
        ("sketch.redefine", json!({"sketch": 1, "plane": -1})),
        ("sketch.redefine", json!({"sketch": 1, "plane": {"x_dir": [1, 0, 0]}})),
        ("sketch.redefine", json!({"sketch": 1, "plane": {"x_dir": [1, 0, 0], "y_dir": [2, 0, 0]}})),
        ("sketch.redefine", json!({"sketch": 1, "plane": {"origin": "1 mm", "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}})),
        ("sketch.redefine", json!({"sketch": 1, "plane": {"face": "x"}})),
        ("sketch.redefine", json!({"sketch": 1, "plane": {"face": [1, 2, 3], "body": "Box"}})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "frame": "local"})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "frame": 3})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "frame": "world"})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "author_path": "1"})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "author_path": null})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "author_path": [0]})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "author_path": ["a"]})),
        ("sketch.redefine", json!({"sketch": 1, "plane": "XZ", "author_path": deep})),
    ]);
}

/// The calls that need a named view to get as far as their message.
#[test]
fn view_strict_keeps_todays_errors_for_views() {
    let long = "n".repeat(129);
    let cases = [
        ("view.rename", json!({"view": "Front 2", "name": ""})),
        ("view.rename", json!({"view": "Front 2", "name": "   "})),
        ("view.rename", json!({"view": "Front 2", "name": 5})),
        ("view.rename", json!({"view": "Front 2", "name": null})),
        ("view.rename", json!({"view": "Front 2", "name": long})),
        ("view.rename", json!({"view": "Front 2"})),
        ("view.rename", json!({"view": "Front 2", "name": "Other"})),
    ];
    for (id, p) in cases {
        let run = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            for name in ["Front 2", "Other"] {
                s.execute("view.save", &json!({"camera": CAMERA(), "name": name})).unwrap_or_else(|e| panic!("{e}"));
            }
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = run(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(run(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// What the handlers refuse is refused by the types with the same message, and what they take
/// is taken: the check names no problem for any call that works today, in the forms that read
/// different keys (`view` or `camera`) and in each form of `plane`.
#[test]
fn the_check_reads_each_key_as_the_handler_does() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    let none = Vec::<String>::new();
    let cam = |extra: Value| {
        let mut c = CAMERA();
        c["distance"] = json!(80);
        for (k, v) in extra.as_object().into_iter().flatten() {
            c[k] = v.clone();
        }
        c
    };
    // view.apply: a view, or a camera.
    assert_eq!(issues("view.apply", json!({"view": "top"})), none);
    assert_eq!(issues("view.apply", json!({"view": "Anything"})), none);
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({}))})), none);
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({"fov": 0.5, "roll": 1.0}))})), none);
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({"roll": 100}))})), none, "a roll past π is clamped");
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({"pitch": std::f64::consts::FRAC_PI_2}))})), none);
    // `view` is read as text only: where `camera` is the form, one that is not text is not looked at.
    assert_eq!(issues("view.apply", json!({"view": null, "camera": cam(json!({}))})), none);
    assert_eq!(issues("view.apply", json!({"view": 5, "camera": cam(json!({}))})), none);
    assert_eq!(issues("view.apply", json!({"view": "top", "camera": cam(json!({}))})), ["give `view` or `camera`, not both"]);
    assert_eq!(issues("view.apply", json!({"view": "top", "camera": null})), ["give `view` or `camera`, not both"]);
    assert_eq!(
        issues("view.apply", json!({"camera": cam(json!({"up": "z"}))})),
        ["camera `up` is not supported: the up axis comes from the design settings"]
    );
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({"roll": "x"}))})), ["camera `roll` must be a finite number in radians"]);
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({"pitch": 2}))})), ["camera `pitch` must be within ±π/2 radians (±90°)"]);
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({"distance": 0}))})), ["the camera must be finite with a positive distance"]);
    assert_eq!(issues("view.apply", json!({"camera": cam(json!({"fov": -1}))})), ["the camera must be finite with a positive distance"]);
    assert!(issues("view.apply", json!({"camera": {"yaw": 1}}))[0].starts_with("bad camera: missing field `target`"));
    // Neither: the handler's own message (the type leaves it to the handler).
    assert_eq!(issues("view.apply", json!({})), none);
    // view.save.
    assert_eq!(issues("view.save", json!({"camera": CAMERA()})), none);
    assert_eq!(issues("view.save", json!({"camera": CAMERA(), "name": "  "})), none);
    assert_eq!(issues("view.save", json!({"camera": CAMERA(), "name": null})), none);
    // Not text: the handler reads it as left out; the check reports it.
    assert_eq!(issues("view.save", json!({"camera": CAMERA(), "name": 5})).len(), 1);
    assert_eq!(
        issues("view.save", json!({"camera": {"name": "old", "target": [1, 2, 3], "yaw": 0, "pitch": 7, "distance": 1, "fov": 0.3}, "name": "N"})),
        none
    );
    assert_eq!(issues("view.save", json!({"camera": cam(json!({"distance": -1}))})), ["the camera must be finite with a positive distance"]);
    assert_eq!(issues("view.save", json!({"camera": CAMERA(), "name": "n".repeat(129)})), ["`name` is too long"]);
    assert_eq!(issues("view.save", json!({"camera": CAMERA(), "name": format!("  {}  ", "n".repeat(128))})), none);
    // view.rename and view.delete.
    assert_eq!(issues("view.rename", json!({"view": "a", "name": "b"})), none);
    assert_eq!(issues("view.rename", json!({"view": 1, "name": "b"})), ["`view` must be a view name"]);
    assert_eq!(issues("view.rename", json!({"view": "a", "name": ""})), ["`name` must be 1…128 characters"]);
    assert_eq!(issues("view.delete", json!({"view": null})), ["`view` must be a view name"]);
    assert_eq!(issues("view.list", json!({})), none);
    assert_eq!(issues("view.list", json!({"views": 1})).len(), 1);
    // sketch.redefine: each form of the plane, and `frame` and `author_path` as given.
    let plane = |p: Value| issues("sketch.redefine", json!({"sketch": "S", "plane": p}));
    for p in [
        json!("XY"),
        json!("front"),
        json!("Plane1"),
        json!(7),
        json!({"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}),
        json!({"origin": [1, 2, 3], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}),
        json!({"origin": null, "x_dir": {"x": 1, "y": 0, "z": 0}, "y_dir": [0, 1, 0]}),
        json!({"face": [1, 2, 3]}),
        json!({"face": [1, 2, 3], "body": "Box", "index": 2, "path": [1, 2], "fixed": true}),
    ] {
        assert_eq!(plane(p.clone()), none, "{p}");
    }
    assert_eq!(plane(json!(1.5)).len(), 1);
    assert_eq!(plane(json!(null)), ["`plane` must be XY, XZ, YZ, a construction plane name or id, or an object"]);
    assert_eq!(plane(json!({"x_dir": [1, 0, 0]})), ["plane needs y_dir"]);
    let with = |extra: Value| {
        let mut p = json!({"sketch": 1, "plane": "XZ"});
        for (k, v) in extra.as_object().into_iter().flatten() {
            p[k] = v.clone();
        }
        issues("sketch.redefine", p)
    };
    assert_eq!(with(json!({"offset": 5})), none);
    assert_eq!(with(json!({"offset": "w / 2"})), none);
    assert_eq!(with(json!({"frame": null})), none);
    assert_eq!(with(json!({"frame": "component", "author_path": [1, 2]})), none);
    assert_eq!(with(json!({"author_path": []})), none);
    assert_eq!(with(json!({"frame": "x"})), ["`frame` must be \"world\" or \"component\""]);
    assert_eq!(with(json!({"author_path": [0]})), ["invalid author_path id"]);
    assert_eq!(with(json!({"author_path": null})), ["author_path must contain at most64 ids"]);
    assert_eq!(issues("sketch.redefine", json!({"sketch": 2.5, "plane": "XZ"})), none);
    assert_eq!(issues("sketch.redefine", json!({"sketch": {}, "plane": "XZ"})), ["`sketch` must be an id or name"]);
}

/// The schemas say what the handlers need, and no more.
#[test]
fn the_view_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    let cam = |extra: Value| {
        let mut c = CAMERA();
        for (k, v) in extra.as_object().into_iter().flatten() {
            c[k] = v.clone();
        }
        c
    };
    takes(
        input_schema_for::<ViewSaveParams>(),
        &[
            (json!({"camera": CAMERA()}), true),
            (json!({"camera": cam(json!({"fov": 0.4})), "name": "N"}), true),
            (json!({"name": "N"}), false),
            (json!({"camera": cam(json!({"distance": 0}))}), false),
            (json!({"camera": {"yaw": 0}}), false),
        ],
    );
    takes(
        input_schema_for::<ViewApplyParams>(),
        &[
            (json!({"view": "top"}), true),
            (json!({"camera": cam(json!({"roll": 0.3, "fov": 0.5}))}), true),
            (json!({}), false),
            (json!({"view": "top", "camera": CAMERA()}), false),
            (json!({"camera": cam(json!({"up": "z"}))}), false),
            (json!({"camera": cam(json!({"pitch": 2}))}), false),
            (json!({"camera": cam(json!({"fov": -1}))}), false),
        ],
    );
    takes(
        input_schema_for::<ViewRenameParams>(),
        &[(json!({"view": "a", "name": "b"}), true), (json!({"view": "a"}), false), (json!({"name": "b"}), false)],
    );
    takes(input_schema_for::<ViewDeleteParams>(), &[(json!({"view": "a"}), true), (json!({}), false), (json!({"view": 1}), false)]);
    takes(input_schema_for::<ViewListParams>(), &[(json!({}), true), (json!({"x": 1}), false)]);
    takes(
        input_schema_for::<RedefineSketchParams>(),
        &[
            (json!({"sketch": 3, "plane": "XZ"}), true),
            (
                json!({"sketch": "S", "plane": {"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "frame": "component", "offset": "2 mm", "author_path": [1]}),
                true,
            ),
            (json!({"sketch": "S", "plane": {"face": [1, 2, 3]}}), true),
            (json!({"plane": "XZ"}), false),
            (json!({"sketch": 3}), false),
            (json!({"sketch": 3, "plane": "XZ", "frame": "x"}), false),
            (json!({"sketch": 3, "plane": "XZ", "author_path": [0]}), false),
        ],
    );
}

/// Every form of the six commands, in Warn: each real reply matches its output schema and
/// carries no warning, and the replies are the JSON the handlers have always returned.
#[test]
fn view_and_redefine_replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value, schema: schemars::Schema| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = schema.as_value();
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        assert!(conforms(schema, schema, &r), "{id} {p}: {r} doesn't match {schema}");
        r
    };
    let saved = run("view.save", json!({"camera": CAMERA()}), output_schema_for::<ViewSaved>());
    assert_eq!(saved, json!({"view": "Named View1"}));
    let saved = run(
        "view.save",
        json!({"camera": {"target": [1, 2, 3], "yaw": 0, "pitch": 0.2, "distance": 9, "fov": 0.4}, "name": " Detail "}),
        output_schema_for::<ViewSaved>(),
    );
    assert_eq!(saved, json!({"view": "Detail"}));
    run("view.save", json!({"camera": CAMERA(), "name": null}), output_schema_for::<ViewSaved>());
    let listed = run("view.list", json!({}), output_schema_for::<ViewListResult>());
    assert_eq!(listed["views"].as_array().map(Vec::len), Some(3), "{listed}");
    assert_eq!(listed["views"][1]["name"], "Detail", "{listed}");
    let schema = output_schema_for::<ViewListResult>();
    let entry = &listed["views"][0];
    assert!(entry["target"].is_array() && entry["fov"] == 0.0 && entry["distance"] == 150.0, "{listed}");
    assert!(conforms(schema.as_value(), schema.as_value(), &listed));
    let renamed = run("view.rename", json!({"view": "Named View1", "name": " Overview "}), output_schema_for::<ViewRenamed>());
    assert_eq!(renamed, json!({"view": "Overview"}));
    let deleted = run("view.delete", json!({"view": "Overview"}), output_schema_for::<ViewDeleted>());
    assert_eq!(deleted, json!({"deleted": "Overview"}));
    let empty = run("view.list", json!({}), output_schema_for::<ViewListResult>());
    assert_eq!(empty["views"].as_array().map(Vec::len), Some(2), "{empty}");

    let camera = output_schema_for::<CameraReply>;
    for view in ["front", "back", "top", "iso", "home"] {
        run("view.apply", json!({"view": view}), camera());
    }
    let named = run("view.apply", json!({"view": "Detail"}), camera());
    assert!((named["camera"]["pitch"].as_f64().unwrap_or_default() - 0.2).abs() < 1e-12, "{named}");
    let turned =
        run("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 1, "pitch": 0.2, "distance": 80, "fov": 0.5, "roll": 1.0}}), camera());
    assert_eq!(turned["camera"]["roll"], 1.0, "{turned}");
    let clamped = run("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 1, "pitch": 0.2, "distance": 80, "roll": 100}}), camera());
    assert!((clamped["camera"]["roll"].as_f64().unwrap_or_default() - std::f64::consts::PI).abs() < 1e-12, "{clamped}");
    run("view.apply", json!({"camera": {"target": [0, 0, 0], "yaw": 0, "pitch": std::f64::consts::FRAC_PI_2, "distance": 80}}), camera());

    // Redefine Sketch: a sketch, moved to a named plane, offset, an explicit plane in each frame,
    // and a face of a body.
    let sketch = |s: &mut Session| s.execute("sketch.create", &json!({"plane": "XY"})).unwrap_or_else(|e| panic!("{e}"))["sketch"].clone();
    let sk = sketch(&mut s);
    let mut run = |id: &str, p: Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        let schema = output_schema_for::<RedefineSketchResult>();
        assert!(r.get("param_warnings").is_none(), "{id} {p}: {r}");
        assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id} {p}: {r}");
        assert!(r["created"].is_object(), "the session adds `created`: {r}");
        let mut r = r;
        r.as_object_mut().map(|o| o.remove("created"));
        r
    };
    let r = run("sketch.redefine", json!({"sketch": sk, "plane": "XZ"}));
    assert_eq!(r, json!({"sketch": sk, "attachment": "fixed"}));
    let r = run("sketch.redefine", json!({"sketch": sk.to_string(), "plane": "YZ", "offset": "5 mm", "frame": null}));
    assert_eq!(r, json!({"sketch": sk, "attachment": "fixed"}));
    let explicit = json!({"origin": [1, 2, 3], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]});
    let r = run("sketch.redefine", json!({"sketch": sk, "plane": explicit}));
    assert_eq!(r, json!({"sketch": sk, "attachment": "fixed", "frame": "world"}));
    let r = run("sketch.redefine", json!({"sketch": sk, "plane": explicit, "frame": "component"}));
    assert_eq!(r, json!({"sketch": sk, "attachment": "fixed", "frame": "component"}));
}

/// A face of a body that comes before the sketch: the sketch follows it, or stays where the face
/// is now with `fixed`.
#[test]
fn redefine_sketch_onto_a_face_matches_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10, "body_name": "Box"})).unwrap_or_else(|e| panic!("{e}"));
    let sk = s.execute("sketch.create", &json!({"plane": "XY"})).unwrap_or_else(|e| panic!("{e}"))["sketch"].clone();
    let mut run = |p: Value| {
        let r = s.execute("sketch.redefine", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        let schema = output_schema_for::<RedefineSketchResult>();
        assert!(r.get("param_warnings").is_none() && conforms(schema.as_value(), schema.as_value(), &r), "{p}: {r}");
        r
    };
    let r = run(json!({"sketch": sk, "plane": {"face": [5, 5, 10]}}));
    assert_eq!(r["attachment"], "exact_face", "{r}");
    let r = run(json!({"sketch": sk, "plane": {"face": [5, 5, 10], "body": "Box", "index": 0, "fixed": true}}));
    assert_eq!(r["attachment"], "fixed", "{r}");
}

/// A moved component: a world plane put into it says so in `note`, which the schema describes.
#[test]
fn redefine_sketch_note_matches_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let part = run("component.create", json!({"name": "Part", "grounded": false}));
    let sk = run("sketch.create", json!({"plane": "XY"}))["sketch"].clone();
    run("occurrence.move", json!({"occurrence": part["occurrence"], "translate": [10, 0, 0]}));
    let r = run("sketch.redefine", json!({"sketch": sk, "plane": {"x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "author_path": [part["occurrence"]]}));
    let schema = output_schema_for::<RedefineSketchResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &r), "{r}");
    assert!(r.get("param_warnings").is_none(), "{r}");
    assert_eq!(r["frame"], "world", "{r}");
    assert!(r["note"].as_str().is_some_and(|n| n.contains("world coordinates")), "{r}");
}

/// A misspelt key still runs in Warn, and says so, with a reply that matches; Strict refuses it.
#[test]
fn a_misspelt_key_is_reported_for_each_view_command() {
    let mut s = Session::default();
    s.execute("view.save", &json!({"camera": CAMERA(), "name": "V"})).unwrap_or_else(|e| panic!("{e}"));
    let sk = s.execute("sketch.create", &json!({"plane": "XY"})).unwrap_or_else(|e| panic!("{e}"))["sketch"].clone();
    s.param_check = ParamCheck::Warn;
    let cases: Vec<(&str, Value, &str, schemars::Schema)> = vec![
        ("view.apply", json!({"view": "top", "camra": 1}), "camra", output_schema_for::<CameraReply>()),
        ("view.list", json!({"view": "V"}), "view", output_schema_for::<ViewListResult>()),
        ("view.rename", json!({"view": "V", "name": "W", "nmae": 1}), "nmae", output_schema_for::<ViewRenamed>()),
        ("view.save", json!({"camera": CAMERA(), "name": "X", "nmae": 1}), "nmae", output_schema_for::<ViewSaved>()),
        ("sketch.redefine", json!({"sketch": sk, "plane": "XZ", "frme": "world"}), "frme", output_schema_for::<RedefineSketchResult>()),
        ("view.delete", json!({"view": "X", "vew": 1}), "vew", output_schema_for::<ViewDeleted>()),
    ];
    for (id, p, key, schema) in cases {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(r["param_warnings"].to_string().contains(key), "{id}: {r}");
        assert!(conforms(schema.as_value(), schema.as_value(), &r), "{id}: {r}");
    }
    s.param_check = ParamCheck::Strict;
    let error = s.execute("view.apply", &json!({"view": "top", "camra": 1})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`camra`"), "{error}");
}

/// The commands run exactly as before with the check off, whatever the call (the types only
/// describe).
#[test]
fn the_view_commands_still_read_their_calls_as_before_with_the_check_off() {
    let mut s = Session::default();
    s.execute("view.save", &json!({"camera": CAMERA(), "name": 5})).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(s.execute("view.list", &json!({"x": 1})).unwrap_or_else(|e| panic!("{e}"))["views"][0]["name"], "Named View1");
    s.execute("view.apply", &json!({"view": 5, "camera": CAMERA()})).unwrap_or_else(|e| panic!("{e}"));
}
