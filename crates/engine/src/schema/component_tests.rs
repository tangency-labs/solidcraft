use solidcraft_geom::prelude::*;
use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    ActivateParams, ActivateResult, CreateParams, CreateResult, DeleteParams, DeleteResult, EditInPlaceParams, FinishEditParams, FromBodiesParams,
    FromBodiesResult, ListParams, ListResult, MirrorComponentsParams, MirrorComponentsResult, MoveFeaturesParams, MoveSketchesToPartsParams,
    OccurrenceAlignParams, OccurrenceAlignResult, PropertiesParams, PropertiesResult, RenameParams, RenameResult, ReparentItemsParams, UnlockParams,
    UnlockResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<EditInPlaceParams>(), &["component", "path"]);
    assert_keys(input_schema_for::<FinishEditParams>(), &[]);
    assert_keys(input_schema_for::<MirrorComponentsParams>(), &["as", "components", "follow", "plane", "symmetry"]);
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
    assert_keys(input_schema_for::<CreateParams>(), &["activate", "grounded", "name", "parent", "parent_path"]);
    assert_keys(input_schema_for::<FromBodiesParams>(), &["bodies"]);
    assert_keys(input_schema_for::<ActivateParams>(), &["component", "occurrence", "path"]);
    assert_keys(input_schema_for::<ListParams>(), &["bodies", "compact"]);
    assert_keys(input_schema_for::<PropertiesParams>(), &["about", "component", "descendants"]);
    assert_keys(input_schema_for::<RenameParams>(), &["component", "name"]);
    assert_keys(input_schema_for::<DeleteParams>(), &["component"]);
    assert_keys(input_schema_for::<UnlockParams>(), &["component"]);
}

/// The calls these commands already refuse (the handlers' own messages), refused alike in strict.
/// An unknown key to `component.create` is left out: its handler lists the accepted parameters,
/// where strict gives the check's `unknown key` with a suggestion (refused either way).
#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("component.create", json!({"parent": null})),
        ("component.create", json!({"parent": true})),
        ("component.create", json!({"parent": [1]})),
        ("component.create", json!({"parent": "Nope"})),
        ("component.create", json!({"parent_path": "1"})),
        ("component.create", json!({"parent_path": null})),
        ("component.create", json!({"parent_path": [1, "a"]})),
        ("component.create", json!({"parent_path": vec![1; 64]})),
        ("component.create", json!({"grounded": "yes"})),
        ("component.create", json!({"grounded": null})),
        ("component.from_bodies", json!({})),
        ("component.from_bodies", json!({"bodies": null})),
        ("component.from_bodies", json!({"bodies": "Box"})),
        ("component.from_bodies", json!({"bodies": [1]})),
        ("component.from_bodies", json!({"bodies": []})),
        ("component.from_bodies", json!({"bodies": ["A", "A"]})),
        ("component.from_bodies", json!({"bodies": vec!["A"; 1001]})),
        ("component.from_bodies", json!({"bodies": ["Nope"]})),
        ("component.activate", json!({})),
        ("component.activate", json!({"component": null})),
        ("component.activate", json!({"component": [1]})),
        ("component.activate", json!({"component": "Nope"})),
        ("component.activate", json!({"component": 0, "path": [1]})),
        ("component.activate", json!({"component": 0, "occurrence": null})),
        ("component.activate", json!({"component": 0, "occurrence": true})),
        ("component.properties", json!({"component": null})),
        ("component.properties", json!({"component": true})),
        ("component.properties", json!({"descendants": "yes"})),
        ("component.properties", json!({"descendants": null})),
        ("component.properties", json!({"about": [1, 2]})),
        ("component.properties", json!({"about": "origin"})),
        ("component.properties", json!({"about": [1, 2, 1e9]})),
        ("component.rename", json!({})),
        ("component.rename", json!({"component": 0})),
        ("component.rename", json!({"name": "A"})),
        ("component.rename", json!({"component": null, "name": "A"})),
        ("component.rename", json!({"component": 0, "name": ""})),
        ("component.rename", json!({"component": 0, "name": " \t "})),
        ("component.rename", json!({"component": 0, "name": 5})),
        ("component.rename", json!({"component": 0, "name": null})),
        ("component.rename", json!({"component": 0, "name": "x".repeat(129)})),
        ("component.delete", json!({})),
        ("component.delete", json!({"component": false})),
        ("component.delete", json!({"component": 0})),
        ("component.unlock", json!({})),
        ("component.unlock", json!({"component": [0]})),
        ("component.unlock", json!({"component": 0})),
        ("component.mirror", json!({"components": ["Part"], "plane": "YZ", "as": "sideways"})),
        ("component.mirror", json!({"components": ["Part"], "plane": "YZ", "as": 3})),
        ("component.mirror", json!({"components": ["Part"], "plane": "YZ", "symmetry": "W"})),
    ]);
}

/// A name of 128 bytes is taken once trimmed, and one of 129 is not, as the handler counts.
#[test]
fn a_name_is_counted_as_the_handler_counts_it() {
    let issues = |p: Value| crate::schema::issues("component.rename", &p).len();
    assert_eq!(issues(json!({"component": 0, "name": format!("  {}  ", "x".repeat(128))})), 0);
    assert_eq!(issues(json!({"component": 0, "name": "x".repeat(129)})), 1);
    // 64 two-byte characters are 128 bytes.
    assert_eq!(issues(json!({"component": 0, "name": "é".repeat(64)})), 0);
    assert_eq!(issues(json!({"component": 0, "name": "é".repeat(65)})), 1);
}

/// The contracts say what the handlers need: a key a handler refuses without is required in the
/// schema, though the type leaves it to the handler's message.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<CreateParams>(),
        &[
            (json!({}), true),
            (json!({"name": "Arm", "parent": "Frame", "parent_path": [1, 2], "activate": false, "grounded": true}), true),
            (json!({"parent": 3, "parent_path": []}), true),
            (json!({"parent": true}), false),
            (json!({"parent_path": vec![1; 64]}), false),
            (json!({"grounded": "yes"}), false),
            (json!({"bodies": ["A"]}), false),
        ],
    );
    takes(
        input_schema_for::<FromBodiesParams>(),
        &[
            (json!({"bodies": ["A", "B"]}), true),
            (json!({}), false),
            (json!({"bodies": []}), false),
            (json!({"bodies": ["A", "A"]}), false),
            (json!({"bodies": "A"}), false),
        ],
    );
    takes(
        input_schema_for::<ActivateParams>(),
        &[
            (json!({"component": 0}), true),
            (json!({"component": "root"}), true),
            (json!({"component": "Arm", "occurrence": "Arm:2", "path": [1, 2]}), true),
            (json!({"component": "Arm", "occurrence": 2}), true),
            (json!({}), false),
            (json!({"component": true}), false),
            (json!({"component": 1, "occurrence": true}), false),
            (json!({"component": 1, "path": []}), false),
        ],
    );
    takes(
        input_schema_for::<ListParams>(),
        &[(json!({}), true), (json!({"compact": true, "bodies": true}), true), (json!({"compact": "yes"}), false)],
    );
    takes(
        input_schema_for::<PropertiesParams>(),
        &[
            (json!({}), true),
            (json!({"component": "Arm", "descendants": false, "about": [1, 2, 3]}), true),
            (json!({"about": {"x": 1, "y": 2, "z": 3}}), true),
            (json!({"about": [1, 2]}), false),
            (json!({"descendants": 1}), false),
        ],
    );
    takes(
        input_schema_for::<RenameParams>(),
        &[(json!({"component": "Arm", "name": "Bar"}), true), (json!({"component": "Arm"}), false), (json!({"name": "Bar"}), false)],
    );
    // `conforms` doesn't read patterns: pin the text that keeps blank names out.
    let rename = input_schema_for::<RenameParams>();
    assert_eq!(rename.as_value()["$defs"]["ComponentName"]["pattern"], crate::schema::types::not_blank_pattern());
    takes(input_schema_for::<DeleteParams>(), &[(json!({"component": 2}), true), (json!({}), false)]);
    takes(input_schema_for::<UnlockParams>(), &[(json!({"component": "rig"}), true), (json!({}), false)]);
}

/// Keys the handlers read only in some cases are read only in those cases: `bodies` of
/// `component.list` with `compact`, `path` of `component.activate` with `occurrence`. And the
/// warnings say what is wrong with a key.
#[test]
fn the_check_reads_keys_as_the_handlers_do() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    let none = Vec::<String>::new();
    assert_eq!(issues("component.list", json!({"bodies": "x"})), none);
    assert_eq!(issues("component.list", json!({"compact": false, "bodies": 3})), none);
    assert_eq!(issues("component.list", json!({"compact": true, "bodies": true})), none);
    assert_eq!(issues("component.list", json!({"compact": true, "bodies": "x"})).len(), 1);
    assert_eq!(issues("component.list", json!({"compact": "x"})).len(), 1);
    assert_eq!(issues("component.list", json!({"compat": true})), ["unknown key `compat`; did you mean `compact`?"]);
    assert_eq!(issues("component.activate", json!({"component": 1, "occurrence": 2})), none);
    assert_eq!(issues("component.activate", json!({"component": 1, "occurrence": 2, "path": [1, 2]})), none);
    assert_eq!(issues("component.activate", json!({"component": 1, "path": [2]})), ["`occurrence` must be an id or name"]);
    assert_eq!(issues("component.activate", json!({"component": 1, "occurrence": 2, "path": []})), ["`path` must list 1..64 occurrence ids"]);
    assert_eq!(issues("component.activate", json!({"component": 1, "occurrence": null})), ["`occurrence` must be an id or name"]);
    assert_eq!(issues("component.create", json!({"parent": true})), ["`parent` must be a component id or name"]);
    assert_eq!(issues("component.create", json!({"parent_path": [1, null]})), ["parent_path must list up to 63 occurrence ids"]);
    assert_eq!(issues("component.create", json!({"grounded": 1})), ["grounded must be true or false"]);
    assert_eq!(issues("component.create", json!({"parent_path": vec![1; 63]})), none);
    assert_eq!(issues("component.create", json!({"parent_path": vec![1; 64]})), ["parent_path must list up to 63 occurrence ids"]);
    assert_eq!(issues("component.create", json!({"activate": "no"})).len(), 1);
    assert_eq!(issues("component.from_bodies", json!({"bodies": [1]})), ["bodies must be an array of body names"]);
    assert_eq!(issues("component.from_bodies", json!({"bodies": ["a", "a"]})), ["bodies must list 1..1000 distinct existing definition bodies"]);
    assert_eq!(issues("component.properties", json!({"about": [1]})), ["`about` must be a point [x, y, z] in mm"]);
    assert_eq!(issues("component.properties", json!({"about": null})), none);
    assert_eq!(issues("component.properties", json!({"descendants": null})), ["`descendants` must be a boolean"]);
    assert_eq!(issues("component.properties", json!({"component": null})), ["`component` must be a component id or name"]);
    assert_eq!(issues("component.rename", json!({"component": 0, "name": ""})), ["`name` must be 1…128 characters"]);
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// A glTF file whose node `rig` holds a `Part` (one cube): imported, it is a component held
/// together by a rigid group that `component.unlock` releases.
fn rig_file(name: &str) -> String {
    let side = 0.01f32;
    let points: Vec<[f32; 3]> = (0..8).map(|i| [(i & 1) as f32 * side, ((i >> 1) & 1) as f32 * side, ((i >> 2) & 1) as f32 * side]).collect();
    let tris: [u16; 36] = [0, 2, 1, 1, 2, 3, 4, 5, 6, 5, 7, 6, 0, 1, 4, 1, 5, 4, 2, 6, 3, 3, 6, 7, 0, 4, 2, 2, 4, 6, 1, 3, 5, 3, 7, 5];
    let mut bin: Vec<u8> = points.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
    bin.extend(tris.iter().flat_map(|v| v.to_le_bytes()));
    let doc = json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "Base", "mesh": 0, "children": [1]}, {"name": "Arm", "mesh": 1, "translation": [0.0, 0.05, 0.0]}],
        "meshes": [{"name": "Block", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]},
                   {"name": "Peg", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [side, side, side]},
            {"bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR"},
        ],
        "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": 96}, {"buffer": 0, "byteOffset": 96, "byteLength": 72}],
        "buffers": [{"byteLength": bin.len()}],
    });
    let path = std::env::temp_dir().join(format!("solidcraft-component-schema-{}-{name}.glb", std::process::id()));
    std::fs::write(&path, solidcraft_io::glb_container(doc.to_string().as_bytes(), &bin)).unwrap_or_else(|e| panic!("{e}"));
    path.to_string_lossy().to_string()
}

/// Every command, in each form it takes, in Warn: the real reply matches its output schema and
/// carries no `param_warnings` (Warn runs a call the contract refuses anyway, so the replies
/// are also checked for warnings), and a key spelt wrongly on purpose is reported.
#[test]
fn replies_match_the_output_schemas_of_the_component_commands() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));

    let arm = run("component.create", json!({"name": "Arm"}));
    assert_eq!(arm["name"], "Arm");
    assert_eq!(arm["grounded"], true, "the design's first part is grounded: {arm}");
    matches(output_schema_for::<CreateResult>(), "component.create", &arm);
    run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "ArmBox"}));
    let hand = run("component.create", json!({"name": "Hand", "parent": "Arm", "activate": false, "grounded": false}));
    assert!(hand["parent"] == arm["component"] && hand["grounded"] == false, "{hand}");
    matches(output_schema_for::<CreateResult>(), "component.create", &hand);
    let at = run("component.create", json!({"name": "Thumb", "parent": hand["parent"], "parent_path": [arm["occurrence"]]}));
    matches(output_schema_for::<CreateResult>(), "component.create", &at);
    let top = run("component.create", json!({"name": "Top", "parent": 0, "parent_path": []}));
    matches(output_schema_for::<CreateResult>(), "component.create", &top);

    let active = run("component.activate", json!({"component": "root"}));
    assert_eq!(active["active"], 0);
    matches(output_schema_for::<ActivateResult>(), "component.activate", &active);
    let active = run("component.activate", json!({"component": arm["component"]}));
    matches(output_schema_for::<ActivateResult>(), "component.activate", &active);
    let active = run("component.activate", json!({"component": "Arm", "occurrence": arm["occurrence"], "path": [arm["occurrence"]]}));
    assert_eq!(active["active"], arm["component"], "{active}");
    matches(output_schema_for::<ActivateResult>(), "component.activate", &active);

    run("solid.box", json!({"length": 5, "width": 5, "height": 5, "body_name": "RootBox"}));
    run("solid.box", json!({"length": 6, "width": 6, "height": 6, "body_name": "RootBox2", "operation": "new"}));
    let made = run("component.from_bodies", json!({"bodies": ["RootBox", "RootBox2"]}));
    assert_eq!(made["components"].as_array().map(Vec::len), Some(2), "{made}");
    matches(output_schema_for::<FromBodiesResult>(), "component.from_bodies", &made);

    for p in [
        json!({}),
        json!({"compact": true}),
        json!({"compact": true, "bodies": true}),
        json!({"compact": false, "bodies": "ignored"}),
        json!({"bodies": true}),
    ] {
        let listed = run("component.list", p.clone());
        matches(output_schema_for::<ListResult>(), "component.list", &listed);
        assert!(listed["components"].as_array().is_some_and(|c| c.len() >= 5), "{p}: {listed}");
    }
    let moved = run("occurrence.move", json!({"occurrence": hand["occurrence"], "translate": [5, 0, 0], "capture": false, "move_grounded": true}));
    assert_eq!(moved["pending"], true, "{moved}");
    run("occurrence.move", json!({"occurrence": top["occurrence"], "translate": [0, 7, 0], "move_grounded": true}));
    let listed = run("component.list", json!({"compact": true}));
    assert!(listed["pending_moves"].as_array().is_some_and(|m| !m.is_empty()), "{listed}");
    // Compact placements carry `transform` only when it is not the identity.
    let placements: Vec<&Value> =
        listed["components"].as_array().into_iter().flatten().flat_map(|c| c["occurrences"].as_array().into_iter().flatten()).collect();
    assert!(placements.iter().any(|o| o.get("transform").is_some()) && placements.iter().any(|o| o.get("transform").is_none()), "{listed}");
    matches(output_schema_for::<ListResult>(), "component.list", &listed);

    for p in [
        json!({}),
        json!({"component": "Arm"}),
        json!({"component": 0, "descendants": false}),
        json!({"component": "root", "about": [10, 0, 0]}),
        json!({"about": {"x": 1, "y": 2, "z": 3}, "descendants": true}),
        json!({"about": null}),
        json!({"component": "Top"}),
    ] {
        let measured = run("component.properties", p.clone());
        matches(output_schema_for::<PropertiesResult>(), "component.properties", &measured);
        assert_eq!(measured["coordinate_frame"], "world", "{p}");
        assert_eq!(measured["total"].get("inertia_about").is_some(), p.get("about").is_some_and(|a| !a.is_null()), "{p}: {measured}");
    }
    let measured = run("component.properties", json!({"component": 0}));
    assert!(measured["body_count"].as_u64().is_some_and(|n| n >= 3) && measured["total"]["bbox"].is_object(), "{measured}");
    let empty = run("component.properties", json!({"component": "Top"}));
    assert!(empty["total"]["bbox"].is_null() && empty["total"]["center_of_mass"].is_null(), "{empty}");

    let renamed = run("component.rename", json!({"component": "Hand", "name": "  Palm "}));
    assert_eq!(renamed["name"], "Palm");
    matches(output_schema_for::<RenameResult>(), "component.rename", &renamed);
    let renamed = run("component.rename", json!({"component": 0, "name": "Whole"}));
    matches(output_schema_for::<RenameResult>(), "component.rename", &renamed);

    let deleted = run("component.delete", json!({"component": "Arm"}));
    assert!(deleted["deleted"].as_array().is_some_and(|d| d.len() >= 3), "{deleted}");
    matches(output_schema_for::<DeleteResult>(), "component.delete", &deleted);

    // Unlock: a glTF node tree is held together by a rigid group.
    let inserted = run("file.insert_mesh", json!({"path": rig_file("rig"), "up": "z"}));
    let rig = inserted["component"].clone();
    let unlocked = run("component.unlock", json!({"component": rig}));
    assert!(unlocked["joints"].as_array().is_some_and(|j| !j.is_empty()), "{unlocked}");
    matches(output_schema_for::<UnlockResult>(), "component.unlock", &unlocked);

    // Misspelt keys still run in Warn, say so, and the reply still matches.
    let cases: [(&str, Value, &str, schemars::Schema); 3] = [
        ("component.list", json!({"compat": true}), "compat", output_schema_for::<ListResult>()),
        ("component.properties", json!({"descendents": false}), "descendents", output_schema_for::<PropertiesResult>()),
        ("component.rename", json!({"component": 0, "nmae": "x", "name": "Again"}), "nmae", output_schema_for::<RenameResult>()),
    ];
    for (id, p, key, schema) in cases {
        let warned = run(id, p);
        let warnings = warned["param_warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.len() == 1 && warnings[0].as_str().is_some_and(|w| w.contains(key)), "{id}: {warned}");
        assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{id}: {warned}");
    }
}

/// An unknown key is refused by `component.create` as before (its own message lists the
/// accepted parameters), at every check level but strict, where the check names the key first.
#[test]
fn create_refuses_an_unknown_key_as_before() {
    for level in ParamCheck::ALL {
        let mut s = Session { param_check: level, ..Session::default() };
        let e = s.execute("component.create", &json!({"nme": "A"})).unwrap_err().to_string();
        assert!(e.contains("unknown parameter `nme`") ^ (level == ParamCheck::Strict), "{level:?}: {e}");
        assert!(e.contains("nme"), "{e}");
    }
    let mut s = Session::default();
    let e = s.execute("component.create", &json!({"bodies": ["A"]})).unwrap_err().to_string();
    assert!(e.contains("use component.move_bodies or component.from_bodies"), "{e}");
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

#[test]
fn mirror_replies_match_the_output_schema() {
    let schema = output_schema_for::<MirrorComponentsResult>();
    let schema = schema.as_value();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 4, "corner": [20, 0, 0]})).unwrap();
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    for p in [
        json!({"components": ["Part"], "plane": "YZ"}),
        json!({"components": "Part:1", "plane": {"origin": [0, 0, 0], "normal": [0, 1, 0]}, "symmetry": "auto", "follow": false}),
        json!({"components": ["Part"], "plane": "XZ", "as": "opposite_hand", "symmetry": "Z"}),
        json!({"components": ["Part"], "plane": "XY", "as": "instance", "follow": true}),
    ] {
        let r = s.execute("component.mirror", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings"), None, "{r}");
    }
    assert!(!conforms(schema, schema, &json!({"occurrences": "1"})));
}

/// A mirror plane placed by a parameter shows in `parameters.users` as the output schema says.
#[test]
fn a_mirror_plane_user_matches_the_parameter_users_schema() {
    let schema = output_schema_for::<crate::schema::parameters::ParamUsersResult>();
    let schema = schema.as_value();
    let mut s = Session::default();
    s.execute("component.create", &json!({"name": "Part", "grounded": false})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 4, "corner": [20, 0, 0]})).unwrap();
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    s.execute("parameters.add", &json!({"name": "gap", "expression": "5 mm"})).unwrap();
    s.execute("component.mirror", &json!({"components": ["Part"], "plane": {"origin": ["gap", 0, 0], "normal": [1, 0, 0]}})).unwrap();
    let users = s.execute("parameters.users", &json!({"name": "gap"})).unwrap();
    assert_eq!(users["users"][0]["type"], "mirror", "{users}");
    assert!(conforms(schema, schema, &users), "{users} doesn't match {schema}");
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
    let world = s.world_state().unwrap();
    let base = world.body("Base").unwrap();
    let top = base.body.faces(0.01).unwrap().into_iter().max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
    let pick = json!({"body": base.name, "index": top.index, "face": [top.centroid.x, top.centroid.y, top.centroid.z], "path": [occurrence]});
    let on_face = run(&mut s, "sketch.create", json!({"name":"OnFace","plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
    sketch_square(&mut s);
    let on_xy = run(&mut s, "sketch.create", json!({"name":"OnXY","plane":"XY"}))["sketch"].as_u64().unwrap();
    sketch_square(&mut s);
    let world_plane = |s: &Session| s.world_state().unwrap().sketch(on_face).unwrap().plane;
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

/// A sketch moved to its part while an exploded view moves the part saves the same picked point
/// and guide plane as without the view: the view is display only.
#[test]
fn sketches_moved_to_a_part_while_an_exploded_view_is_shown_save_what_they_save_without_it() {
    let run = |s: &mut Session, c: &str, p: Value| s.execute(c, &p).unwrap_or_else(|e| panic!("{c}: {e}"));
    let moved = |exploded: bool| {
        let mut s = Session::default();
        let part = run(&mut s, "component.create", json!({"name":"Part","parent":0,"grounded":false}));
        let occurrence = part["occurrence"].as_u64().unwrap();
        run(&mut s, "solid.box", json!({"length":10,"width":10,"height":10,"body_name":"Base"}));
        run(&mut s, "occurrence.move", json!({"occurrence":occurrence,"translate":[30,0,0]}));
        run(&mut s, "component.activate", json!({"component":0}));
        let world = s.world_state().unwrap();
        let base = world.body("Base").unwrap();
        let top = base.body.faces(0.01).unwrap().into_iter().max_by(|a, b| a.centroid.z.total_cmp(&b.centroid.z)).unwrap();
        let pick = json!({"body": base.name, "index": top.index, "face": [top.centroid.x, top.centroid.y, top.centroid.z], "path": [occurrence]});
        let on_face = run(&mut s, "sketch.create", json!({"name":"OnFace","plane":pick,"project_edges":false}))["sketch"].as_u64().unwrap();
        sketch_square(&mut s);
        // An exploded view needs two placements: B stays put, in both runs.
        run(&mut s, "component.create", json!({"name":"B","parent":0,"activate":false}));
        if exploded {
            let view =
                run(&mut s, "explode.create", json!({"moves": [{"occurrence": occurrence, "translate": [0, 25, 0]}]}))["view"].as_u64().unwrap();
            run(&mut s, "explode.show", json!({"view": view}));
        }
        run(&mut s, "component.move_sketches.to_parts", json!({}));
        run(&mut s, "explode.show", json!({}));
        let solidcraft_doc::FeatureKind::Sketch { plane: solidcraft_doc::PlaneRef::Face { plane, at, .. }, .. } =
            &s.doc.feature(on_face).unwrap().kind
        else {
            panic!("not a face sketch")
        };
        (*plane, *at)
    };
    let ((plane, at), (assembled_plane, assembled_at)) = (moved(true), moved(false));
    assert!(at.dist(assembled_at) < 1e-9, "{at:?} vs {assembled_at:?}");
    assert!(plane.origin.dist(assembled_plane.origin) < 1e-9, "{plane:?} vs {assembled_plane:?}");
}
