use solidcraft_geom::prelude::*;
use std::sync::Arc;

use serde_json::{Value, json};

use super::{
    ActivateParams, ActivateResult, CapturePositionParams, CapturePositionResult, CopyParams, CopyResult, CreateParams, CreateResult, DeleteParams,
    DeleteResult, DragParams, DragResult, EditInPlaceParams, FinishEditParams, FromBodiesParams, FromBodiesResult, GroundParams, GroundResult,
    ListParams, ListResult, MirrorComponentsParams, MirrorComponentsResult, MoveBodiesParams, MoveBodiesResult, MoveFeaturesParams, MoveParams,
    MoveResult, MoveSketchesParams, MoveSketchesResult, MoveSketchesToPartsParams, OccurrenceAlignParams, OccurrenceAlignResult,
    OccurrencePropertiesParams, OccurrencePropertiesResult, PasteNewParams, PasteNewResult, PropertiesParams, PropertiesResult, RenameParams,
    RenameResult, ReparentItemsParams, RevertPositionParams, RevertPositionResult, UnlockParams, UnlockResult,
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
    assert_keys(input_schema_for::<CapturePositionParams>(), &[]);
    assert_keys(input_schema_for::<RevertPositionParams>(), &["occurrences"]);
    assert_keys(input_schema_for::<GroundParams>(), &["grounded", "occurrence", "path"]);
    assert_keys(input_schema_for::<CopyParams>(), &["component", "in_place", "occurrence", "parent", "parent_path", "path", "translate"]);
    assert_keys(input_schema_for::<PasteNewParams>(), &["component", "in_place", "occurrence", "parent", "parent_path", "path", "translate"]);
    assert_keys(
        input_schema_for::<MoveParams>(),
        &["angle", "axis", "base_transform", "capture", "matrix", "move_grounded", "occurrence", "origin", "path", "space", "tolerance", "translate"],
    );
    assert_keys(input_schema_for::<DragParams>(), &["at", "continuous", "occurrence", "path", "point"]);
    assert_keys(input_schema_for::<OccurrencePropertiesParams>(), &["about", "descendants", "occurrence", "path"]);
    assert_keys(input_schema_for::<MoveBodiesParams>(), &["bodies", "component", "component_path", "from_path"]);
    assert_keys(input_schema_for::<MoveSketchesParams>(), &["component", "sketches"]);
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

// ---------------------------------------------------------------------------------------------
// The occurrence commands, Capture and Revert Position, Move Bodies and Sketches to Component and
// Paste New (#13186).
// ---------------------------------------------------------------------------------------------

/// Two ungrounded parts, `A` (with a box, `ABox`, and a sketch, `S`) and an empty `B`, in the
/// design: the session at `level`, and the ids of their placements.
fn two_parts(level: ParamCheck) -> (Session, u64, u64) {
    let mut s = Session { param_check: level, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let a = run("component.create", json!({"name": "A", "parent": 0, "grounded": false}))["occurrence"].as_u64().unwrap_or(0);
    run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "ABox"}));
    run("sketch.create", json!({"name": "S", "plane": "XY"}));
    run("sketch.rectangle.two_point", json!({"p0": [1, 1], "p1": [3, 3]}));
    run("sketch.finish", json!({}));
    run("component.activate", json!({"component": 0}));
    let b = run("component.create", json!({"name": "B", "parent": 0, "grounded": false, "activate": false}))["occurrence"].as_u64().unwrap_or(0);
    (s, a, b)
}

/// Each case fails today (the handler's message, in Off) and fails the same in strict.
fn strict_keeps(cases: impl Fn(u64, u64) -> Vec<(&'static str, Value)>) {
    let (_, a, b) = two_parts(ParamCheck::Off);
    for (id, p) in cases(a, b) {
        let error = |level: ParamCheck| two_parts(level).0.execute(id, &p).err().map(|e| e.to_string());
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

const FLAT_MATRIX: [i32; 12] = [1, 0, 0, 5, 0, 1, 0, 0, 0, 0, 1, 0];

/// What the occurrence commands already refuse (their handlers' own messages), refused alike in
/// strict, including the keys they read only in some cases.
#[test]
fn strict_keeps_todays_errors_of_the_occurrence_commands() {
    strict_keeps(|a, b| {
        let at = json!([0, 0, 0]);
        vec![
            ("component.revert_position", json!({"occurrences": null})),
            ("component.revert_position", json!({"occurrences": "1"})),
            ("component.revert_position", json!({"occurrences": []})),
            ("component.revert_position", json!({"occurrences": [a, "x"]})),
            ("component.revert_position", json!({"occurrences": vec![1; 10_001]})),
            ("component.revert_position", json!({"occurrences": [999]})),
            ("occurrence.ground", json!({})),
            ("occurrence.ground", json!({"occurrence": null})),
            ("occurrence.ground", json!({"occurrence": true})),
            ("occurrence.ground", json!({"occurrence": "Nope"})),
            ("occurrence.ground", json!({"occurrence": a, "path": null})),
            ("occurrence.ground", json!({"occurrence": a, "path": "x"})),
            ("occurrence.ground", json!({"occurrence": a, "path": []})),
            ("occurrence.ground", json!({"occurrence": a, "path": vec![a; 65]})),
            ("occurrence.copy", json!({})),
            ("occurrence.copy", json!({"component": null})),
            ("occurrence.copy", json!({"component": [1]})),
            ("occurrence.copy", json!({"occurrence": null})),
            ("occurrence.copy", json!({"occurrence": true, "component": "A"})),
            ("occurrence.copy", json!({"occurrence": a, "path": null})),
            ("occurrence.copy", json!({"occurrence": a, "path": []})),
            ("occurrence.copy", json!({"occurrence": a, "parent": null})),
            ("occurrence.copy", json!({"occurrence": a, "parent": true})),
            ("occurrence.copy", json!({"occurrence": a, "parent": "Nope"})),
            ("occurrence.copy", json!({"occurrence": a, "translate": [1, 2]})),
            ("occurrence.copy", json!({"occurrence": a, "translate": null})),
            ("occurrence.copy", json!({"occurrence": a, "translate": [1e9, 0, 0]})),
            ("occurrence.copy", json!({"occurrence": a, "in_place": true, "parent_path": "x"})),
            ("occurrence.copy", json!({"occurrence": a, "in_place": true, "parent_path": vec![1; 65]})),
            ("component.paste_new", json!({})),
            ("component.paste_new", json!({"component": null})),
            ("component.paste_new", json!({"component": true})),
            ("component.paste_new", json!({"component": 0})),
            ("component.paste_new", json!({"component": "A", "occurrence": null})),
            ("component.paste_new", json!({"component": "A", "occurrence": b})),
            ("component.paste_new", json!({"component": "A", "occurrence": a, "path": []})),
            ("component.paste_new", json!({"component": "A", "translate": [1]})),
            ("component.paste_new", json!({"component": "A", "parent": true})),
            ("component.paste_new", json!({"component": "A", "parent": "A"})),
            ("component.paste_new", json!({"component": "A", "in_place": true, "parent_path": 5})),
            ("occurrence.move", json!({})),
            ("occurrence.move", json!({"occurrence": null})),
            ("occurrence.move", json!({"occurrence": [1]})),
            ("occurrence.move", json!({"occurrence": a, "translate": [1, 2]})),
            ("occurrence.move", json!({"occurrence": a, "translate": null})),
            ("occurrence.move", json!({"occurrence": a, "translate": [1e9, 0, 0]})),
            ("occurrence.move", json!({"occurrence": a, "axis": "z"})),
            ("occurrence.move", json!({"occurrence": a, "origin": [1, 2, "x"]})),
            ("occurrence.move", json!({"occurrence": a, "angle": true})),
            ("occurrence.move", json!({"occurrence": a, "angle": null})),
            ("occurrence.move", json!({"occurrence": a, "angle": 1, "axis": [0, 0, 0]})),
            ("occurrence.move", json!({"occurrence": a, "space": "up"})),
            ("occurrence.move", json!({"occurrence": a, "space": "World"})),
            ("occurrence.move", json!({"occurrence": a, "space": null})),
            ("occurrence.move", json!({"occurrence": a, "path": []})),
            ("occurrence.move", json!({"occurrence": a, "path": null})),
            ("occurrence.move", json!({"occurrence": a, "base_transform": [[1]]})),
            ("occurrence.move", json!({"occurrence": a, "base_transform": [[1, 0, 0, 5], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]})),
            ("occurrence.move", json!({"occurrence": a, "base_transform": [[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 1]]})),
            ("occurrence.move", json!({"occurrence": a, "matrix": null})),
            ("occurrence.move", json!({"occurrence": a, "matrix": [1, 2, 3]})),
            ("occurrence.move", json!({"occurrence": a, "matrix": FLAT_MATRIX, "translate": [1, 0, 0]})),
            ("occurrence.move", json!({"occurrence": a, "matrix": FLAT_MATRIX, "axis": null})),
            ("occurrence.move", json!({"occurrence": a, "matrix": FLAT_MATRIX, "angle": 1})),
            ("occurrence.move", json!({"occurrence": a, "matrix": FLAT_MATRIX, "origin": at})),
            ("occurrence.move", json!({"occurrence": a, "matrix": FLAT_MATRIX, "tolerance": 0})),
            ("occurrence.move", json!({"occurrence": a, "matrix": FLAT_MATRIX, "tolerance": "x"})),
            ("occurrence.move", json!({"occurrence": a, "matrix": FLAT_MATRIX, "tolerance": 0.01})),
            ("occurrence.move", json!({"occurrence": a, "matrix": [2, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]})),
            ("occurrence.move", json!({"occurrence": a, "matrix": [-1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]})),
            ("occurrence.move", json!({"occurrence": a, "matrix": [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1]})),
            ("occurrence.drag", json!({})),
            ("occurrence.drag", json!({"path": "x", "at": at, "point": at})),
            ("occurrence.drag", json!({"path": [], "at": at, "point": at})),
            ("occurrence.drag", json!({"occurrence": null, "at": at, "point": at})),
            ("occurrence.drag", json!({"occurrence": a})),
            ("occurrence.drag", json!({"occurrence": a, "at": at})),
            ("occurrence.drag", json!({"occurrence": a, "point": at})),
            ("occurrence.drag", json!({"occurrence": a, "at": 5})),
            ("occurrence.drag", json!({"occurrence": a, "path": null, "at": at, "point": at})),
            ("occurrence.drag", json!({"occurrence": a, "path": [], "at": at, "point": at})),
            ("occurrence.drag", json!({"occurrence": a, "at": null, "point": at})),
            ("occurrence.drag", json!({"occurrence": a, "at": at, "point": [1, 2]})),
            ("occurrence.drag", json!({"occurrence": a, "at": [1e9, 0, 0], "point": at})),
            ("occurrence.properties", json!({})),
            ("occurrence.properties", json!({"occurrence": null})),
            ("occurrence.properties", json!({"occurrence": false})),
            ("occurrence.properties", json!({"occurrence": a, "path": []})),
            ("occurrence.properties", json!({"occurrence": a, "path": null})),
            ("occurrence.properties", json!({"occurrence": a, "descendants": "yes"})),
            ("occurrence.properties", json!({"occurrence": a, "descendants": null})),
            ("occurrence.properties", json!({"occurrence": a, "about": [1, 2]})),
            ("occurrence.properties", json!({"occurrence": a, "about": "origin"})),
            ("component.move_bodies", json!({})),
            ("component.move_bodies", json!({"bodies": ["ABox"]})),
            ("component.move_bodies", json!({"component": null, "bodies": ["ABox"]})),
            ("component.move_bodies", json!({"component": "B"})),
            ("component.move_bodies", json!({"component": "B", "bodies": null})),
            ("component.move_bodies", json!({"component": "B", "bodies": "ABox"})),
            ("component.move_bodies", json!({"component": "B", "bodies": [1]})),
            ("component.move_bodies", json!({"component": "B", "bodies": []})),
            ("component.move_bodies", json!({"component": "B", "bodies": ["ABox", "ABox"]})),
            ("component.move_bodies", json!({"component": "B", "bodies": vec!["ABox"; 1001]})),
            ("component.move_bodies", json!({"component": "B", "bodies": ["Nope"]})),
            ("component.move_bodies", json!({"component": "Nope", "bodies": ["ABox"]})),
            ("component.move_bodies", json!({"component": "B", "bodies": ["ABox"], "component_path": null})),
            ("component.move_bodies", json!({"component": "B", "bodies": ["ABox"], "component_path": "x"})),
            ("component.move_bodies", json!({"component": "B", "bodies": ["ABox"], "component_path": vec![b; 65]})),
            ("component.move_bodies", json!({"component": "B", "bodies": ["ABox"], "from_path": [b, "x"]})),
            ("component.move_sketches", json!({})),
            ("component.move_sketches", json!({"sketches": ["S"]})),
            ("component.move_sketches", json!({"component": null, "sketches": ["S"]})),
            ("component.move_sketches", json!({"component": true, "sketches": ["S"]})),
            ("component.move_sketches", json!({"component": "B"})),
            ("component.move_sketches", json!({"component": "B", "sketches": null})),
            ("component.move_sketches", json!({"component": "B", "sketches": []})),
            ("component.move_sketches", json!({"component": "B", "sketches": 5})),
            ("component.move_sketches", json!({"component": "B", "sketches": [1]})),
            ("component.move_sketches", json!({"component": "B", "sketches": ["S", true]})),
            ("component.move_sketches", json!({"component": "B", "sketches": vec!["S"; 10_001]})),
            ("component.move_sketches", json!({"component": "B", "sketches": ["Nope"]})),
        ]
    });
}

/// The contracts say what the handlers need, in the schemas: required keys are required though the
/// types leave them to the handlers' messages, and the ways a command can be called are open.
#[test]
fn the_schemas_take_what_the_occurrence_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    let matrix: Vec<i32> = FLAT_MATRIX.to_vec();
    takes(input_schema_for::<CapturePositionParams>(), &[(json!({}), true), (json!({"x": 1}), false)]);
    takes(
        input_schema_for::<RevertPositionParams>(),
        &[(json!({}), true), (json!({"occurrences": [1, 2]}), true), (json!({"occurrences": []}), false), (json!({"occurrences": ["1"]}), false)],
    );
    takes(
        input_schema_for::<GroundParams>(),
        &[
            (json!({"occurrence": 1}), true),
            (json!({"occurrence": "A:1", "path": [1], "grounded": false}), true),
            (json!({}), false),
            (json!({"occurrence": 1, "path": []}), false),
            (json!({"occurrence": true}), false),
        ],
    );
    takes(
        input_schema_for::<CopyParams>(),
        &[
            (json!({"occurrence": 1}), true),
            (json!({"component": "A"}), true),
            (json!({"occurrence": 1, "translate": {"x": 1, "y": 2, "z": 3}, "in_place": true, "parent_path": [], "path": [1], "parent": 0}), true),
            (json!({}), false),
            (json!({"occurrence": 1, "translate": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<PasteNewParams>(),
        &[
            (json!({"component": "A"}), true),
            (
                json!({"component": 1, "occurrence": 2, "path": [2], "parent": "B", "translate": [1, 2, 3], "in_place": true, "parent_path": [1]}),
                true,
            ),
            (json!({"occurrence": 1}), false),
            (json!({"component": "A", "translate": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<MoveParams>(),
        &[
            (json!({"occurrence": 1}), true),
            (json!({"occurrence": 1, "translate": [1, 2, 3], "axis": [0, 0, 1], "angle": "30 deg", "origin": [0, 0, 0], "space": "world"}), true),
            (json!({"occurrence": 1, "angle": 0.5, "capture": false, "move_grounded": true, "path": [1]}), true),
            (json!({"occurrence": 1, "matrix": matrix, "tolerance": 1e-6}), true),
            (json!({"occurrence": 1, "matrix": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]}), true),
            (json!({"occurrence": 1, "base_transform": [[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]}), true),
            (json!({}), false),
            (json!({"occurrence": 1, "matrix": FLAT_MATRIX, "translate": [1, 0, 0]}), false),
            (json!({"occurrence": 1, "matrix": FLAT_MATRIX, "origin": [1, 0, 0]}), false),
            (json!({"occurrence": 1, "matrix": [1, 2, 3]}), false),
            (json!({"occurrence": 1, "space": "up"}), false),
            (json!({"occurrence": 1, "angle": true}), false),
        ],
    );
    takes(
        input_schema_for::<DragParams>(),
        &[
            (json!({"occurrence": 1, "at": [0, 0, 0], "point": [1, 0, 0]}), true),
            (json!({"path": [1, 2], "at": {"x": 0, "y": 0, "z": 0}, "point": [1, 0, 0], "continuous": true}), true),
            (json!({"at": [0, 0, 0], "point": [1, 0, 0]}), false),
            (json!({"occurrence": 1, "at": [0, 0, 0]}), false),
            (json!({"occurrence": 1, "point": [0, 0, 0]}), false),
            (json!({"occurrence": 1, "path": [], "at": [0, 0, 0], "point": [1, 0, 0]}), false),
        ],
    );
    takes(
        input_schema_for::<OccurrencePropertiesParams>(),
        &[
            (json!({"occurrence": 1}), true),
            (json!({"occurrence": "A:1", "path": [1], "descendants": false, "about": [1, 2, 3]}), true),
            (json!({"occurrence": 1, "about": null}), true),
            (json!({}), false),
            (json!({"occurrence": 1, "descendants": 1}), false),
            (json!({"occurrence": 1, "about": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<MoveBodiesParams>(),
        &[
            (json!({"component": "B", "bodies": ["A"]}), true),
            (json!({"component": 2, "bodies": ["A", "B"], "component_path": [1], "from_path": []}), true),
            (json!({"bodies": ["A"]}), false),
            (json!({"component": "B"}), false),
            (json!({"component": "B", "bodies": "A"}), false),
            (json!({"component": "B", "bodies": ["A"], "from_path": vec![1; 65]}), false),
        ],
    );
    takes(
        input_schema_for::<MoveSketchesParams>(),
        &[
            (json!({"component": "B", "sketches": ["S"]}), true),
            (json!({"component": "B", "sketches": "S"}), true),
            (json!({"component": 2, "sketches": ["S", "T"]}), true),
            (json!({"sketches": ["S"]}), false),
            (json!({"component": "B"}), false),
            (json!({"component": "B", "sketches": []}), false),
            (json!({"component": "B", "sketches": [1]}), false),
        ],
    );
}

/// The types read each key as its handler does: keys the handlers read only in some cases are
/// read only in those, and a wrong key says what is wrong with it in the handler's words.
#[test]
fn the_check_reads_the_occurrence_keys_as_the_handlers_do() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    let none = Vec::<String>::new();
    // `occurrence.copy`: `component` only without `occurrence`, `parent_path` only with `in_place: true`.
    assert_eq!(issues("occurrence.copy", json!({"occurrence": 1, "component": [1], "parent_path": "x"})), none);
    assert_eq!(issues("occurrence.copy", json!({"component": [1]})), ["`component` must be a component id or name"]);
    assert_eq!(issues("occurrence.copy", json!({"occurrence": null, "component": 1})), ["`occurrence` must be an id or name"]);
    assert_eq!(issues("occurrence.copy", json!({"occurrence": 1, "in_place": false, "parent_path": "x"})), none);
    assert_eq!(
        issues("occurrence.copy", json!({"occurrence": 1, "in_place": true, "parent_path": "x"})),
        ["`parent_path` must list 0..64 occurrence ids"]
    );
    assert_eq!(issues("occurrence.copy", json!({"occurrence": 1, "in_place": true, "parent_path": []})), none);
    assert_eq!(issues("occurrence.copy", json!({"occurrence": 1, "path": []})), ["`path` must list 1..64 occurrence ids"]);
    assert_eq!(issues("occurrence.copy", json!({"occurrence": 1, "translate": [1, 2]})), ["translate must contain three finite numbers"]);
    assert_eq!(issues("occurrence.copy", json!({"occurrence": 1, "in_place": "yes"})).len(), 1);
    assert_eq!(issues("occurrence.copy", json!({"occurence": 1})), ["unknown key `occurence`; did you mean `occurrence`?"]);
    // `component.paste_new`: `parent_path` only with `in_place: true`.
    assert_eq!(issues("component.paste_new", json!({"component": 1, "parent_path": 5})), none);
    assert_eq!(
        issues("component.paste_new", json!({"component": 1, "in_place": true, "parent_path": 5})),
        ["`parent_path` must list 0..64 occurrence ids"]
    );
    assert_eq!(issues("component.paste_new", json!({"component": 1, "occurrence": 2, "path": [2], "translate": {"x": 1, "y": 2, "z": 3}})), none);
    assert_eq!(issues("component.paste_new", json!({"component": 1, "path": []})), ["`path` must list 1..64 occurrence ids"]);
    assert_eq!(issues("component.paste_new", json!({"component": 1, "parent": null})), ["`parent` must be a component id or name"]);
    // `occurrence.move`: `tolerance` only with `matrix`, which takes the place of the move keys.
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "tolerance": "x"})), none);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "matrix": FLAT_MATRIX, "tolerance": 1e-6})), none);
    assert_eq!(
        issues("occurrence.move", json!({"occurrence": 1, "matrix": FLAT_MATRIX, "tolerance": "x"})),
        ["tolerance must be a number above 0 and at most 0.001"]
    );
    assert_eq!(
        issues("occurrence.move", json!({"occurrence": 1, "matrix": FLAT_MATRIX, "translate": "x"})),
        ["give either matrix or translate, not both"]
    );
    assert_eq!(
        issues("occurrence.move", json!({"occurrence": 1, "matrix": FLAT_MATRIX, "origin": null})),
        ["give either matrix or origin, not both"]
    );
    assert!(issues("occurrence.move", json!({"occurrence": 1, "matrix": [-1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]}))[0].starts_with("matrix mirrors"));
    assert_eq!(
        issues("occurrence.move", json!({"occurrence": 1, "matrix": [1]})),
        ["matrix must be 12 numbers (3x4, row by row) or 16 (4x4), flat or as rows"]
    );
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "translate": [1, 2]})), ["translate must contain three finite numbers"]);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "axis": null})), ["axis must contain three finite numbers"]);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "origin": "x"})), ["origin must contain three finite numbers"]);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "angle": null})), ["angle must be a number or angle expression"]);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "angle": "30 deg", "axis": {"x": 0, "y": 1, "z": 0}})), none);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "space": "World"})), ["space must be local or world"]);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "base_transform": [[1]]})), ["base_transform must be a 4x4 affine matrix"]);
    assert_eq!(
        issues("occurrence.move", json!({"occurrence": 1, "base_transform": [[1, 0, 0, 2], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]})),
        ["base_transform must be finite, affine and invertible"]
    );
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "capture": "no"})).len(), 1);
    assert_eq!(issues("occurrence.move", json!({"occurrence": 1, "translte": [1, 0, 0]})), ["unknown key `translte`; did you mean `translate`?"]);
    // `occurrence.drag`: `path` stands in for `occurrence`, and `at` and `point` come together.
    let at = json!([0, 0, 0]);
    assert_eq!(issues("occurrence.drag", json!({"path": [1], "at": at, "point": at})), none);
    assert_eq!(issues("occurrence.drag", json!({"at": at, "point": at})), ["give `occurrence` or `path` (1..64 occurrence ids)"]);
    assert_eq!(issues("occurrence.drag", json!({"path": [], "at": at, "point": at})), ["give `occurrence` or `path` (1..64 occurrence ids)"]);
    assert_eq!(issues("occurrence.drag", json!({"occurrence": 1, "path": [], "at": at, "point": at})), ["`path` must list 1..64 occurrence ids"]);
    assert_eq!(issues("occurrence.drag", json!({"occurrence": 1, "at": at})), ["`at` and `point` must both be given"]);
    assert_eq!(issues("occurrence.drag", json!({"occurrence": 1, "at": 5})), ["`at` and `point` must both be given"]);
    assert_eq!(issues("occurrence.drag", json!({"occurrence": 1, "at": 5, "point": at})), ["at must contain three finite numbers"]);
    assert_eq!(issues("occurrence.drag", json!({"occurrence": 1, "at": at, "point": [1]})), ["point must contain three finite numbers"]);
    // The rest.
    assert_eq!(issues("component.revert_position", json!({"occurrences": []})), ["occurrences must list 1..10000 ids"]);
    assert_eq!(issues("component.revert_position", json!({"occurrences": null})), ["occurrences must list 1..10000 ids"]);
    assert_eq!(issues("component.capture_position", json!({"capture": true})), ["unknown key `capture`"]);
    assert_eq!(issues("occurrence.ground", json!({"occurrence": 1, "grounded": null})), none);
    assert_eq!(issues("occurrence.ground", json!({"occurrence": 1, "path": []})), ["`path` must list 1..64 occurrence ids"]);
    assert_eq!(issues("occurrence.properties", json!({"occurrence": 1, "about": [1]})), ["`about` must be a point [x, y, z] in mm"]);
    assert_eq!(issues("occurrence.properties", json!({"occurrence": 1, "descendants": null})), ["`descendants` must be a boolean"]);
    assert_eq!(issues("component.move_bodies", json!({"component": 1, "bodies": "A"})), ["bodies must be an array of body names"]);
    assert_eq!(
        issues("component.move_bodies", json!({"component": 1, "bodies": ["A"], "from_path": null})),
        ["`from_path` must list 0..64 occurrence ids"]
    );
    assert_eq!(issues("component.move_sketches", json!({"component": 1, "sketches": 5})), ["`sketches` must be a list of names; it is 5"]);
    assert_eq!(issues("component.move_sketches", json!({"component": 1, "sketches": ["S", 3]})), ["`sketches` must list names; the entry 2 is 3"]);
    assert_eq!(issues("component.move_sketches", json!({"component": 1, "sketches": null})), ["`sketches` must list sketches"]);
    assert_eq!(issues("component.move_sketches", json!({"component": 1, "sketches": "S"})), none);
}

/// A part set up with its placements: the design's grounded `Base`, a free `Plate` (a box, and
/// the sketch `S`) and an empty free `Spare`.
fn plates(s: &mut Session) -> (Value, Value, Value) {
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let base = run("component.create", json!({"name": "Base"}));
    run("solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "BaseBox"}));
    run("component.activate", json!({"component": 0}));
    let plate = run("component.create", json!({"name": "Plate", "parent": 0, "grounded": false}));
    run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "PlateBox"}));
    run("sketch.create", json!({"name": "S", "plane": "XY"}));
    run("sketch.rectangle.two_point", json!({"p0": [21, 1], "p1": [23, 3]}));
    run("sketch.finish", json!({}));
    run("component.activate", json!({"component": 0}));
    let spare = run("component.create", json!({"name": "Spare", "parent": 0, "grounded": false, "activate": false}));
    (base, plate, spare)
}

/// Every command, in each form it takes, in Warn: the real reply matches its output schema and
/// carries no `param_warnings` (Warn runs a call the contract refuses anyway, so the replies are
/// also checked for warnings), and a key spelt wrongly on purpose is reported.
#[test]
fn replies_match_the_output_schemas_of_the_occurrence_commands() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let (base, plate, spare) = plates(&mut s);
    let (base_at, pl, spare_at) = (base["occurrence"].clone(), plate["occurrence"].clone(), spare["occurrence"].clone());
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));

    // The placement's name, to give it by name.
    let listed = run("component.list", json!({}));
    let plate_name = listed["components"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|c| c["occurrences"].as_array().into_iter().flatten())
        .find(|o| o["id"] == pl)
        .map(|o| o["name"].clone())
        .unwrap_or_default();
    assert!(plate_name.is_string(), "{listed}");

    // Move Occurrence, in each form.
    let identity = json!([[1, 0, 0, 0], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]);
    for p in [
        json!({"occurrence": pl, "translate": [1, 0, 0]}),
        json!({"occurrence": pl, "translate": {"x": 0, "y": 1, "z": 0}, "space": "local"}),
        json!({"occurrence": pl, "space": "world", "path": [pl], "translate": [0, 0, 1]}),
        json!({"occurrence": pl, "path": [pl], "translate": [0, 0, -1]}),
        json!({"occurrence": pl, "axis": [0, 0, 1], "angle": "90 deg", "origin": [0, 0, 0]}),
        json!({"occurrence": pl, "axis": {"x": 0, "y": 1, "z": 0}, "angle": 0.5, "origin": {"x": 1, "y": 1, "z": 1}}),
        json!({"occurrence": pl, "angle": 0}),
        json!({"occurrence": pl, "tolerance": "ignored without a matrix"}),
        json!({"occurrence": pl, "matrix": FLAT_MATRIX}),
        json!({"occurrence": pl, "matrix": FLAT_MATRIX, "tolerance": 1e-6, "space": "world", "path": [pl]}),
        json!({"occurrence": pl, "matrix": [[1, 0, 0, 1], [0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]]}),
        json!({"occurrence": pl, "base_transform": identity, "translate": [3, 0, 0]}),
        json!({"occurrence": pl, "capture": false, "translate": [0, 2, 0]}),
        json!({"occurrence": base_at, "translate": [0, 0, 0], "move_grounded": true}),
    ] {
        let moved = run("occurrence.move", p.clone());
        matches(output_schema_for::<MoveResult>(), "occurrence.move", &moved);
        assert_eq!(moved["pending"], p.get("capture") == Some(&json!(false)), "{p}: {moved}");
    }

    // Capture and Revert Position.
    let captured = run("component.capture_position", json!({}));
    assert_eq!(captured["captured"], 1, "{captured}");
    matches(output_schema_for::<CapturePositionResult>(), "component.capture_position", &captured);
    let captured = run("component.capture_position", json!({}));
    assert_eq!(captured["captured"], 0, "{captured}");
    matches(output_schema_for::<CapturePositionResult>(), "component.capture_position", &captured);
    run("occurrence.move", json!({"occurrence": pl, "capture": false, "translate": [0, 2, 0]}));
    let reverted = run("component.revert_position", json!({"occurrences": [pl]}));
    assert_eq!(reverted["reverted"], 1, "{reverted}");
    matches(output_schema_for::<RevertPositionResult>(), "component.revert_position", &reverted);
    run("occurrence.move", json!({"occurrence": pl, "capture": false, "translate": [0, 2, 0]}));
    let reverted = run("component.revert_position", json!({}));
    assert_eq!(reverted["reverted"], 1, "{reverted}");
    matches(output_schema_for::<RevertPositionResult>(), "component.revert_position", &reverted);

    // Ground.
    for p in [
        json!({"occurrence": pl}),
        json!({"occurrence": pl, "grounded": true, "path": [pl]}),
        json!({"occurrence": pl, "grounded": null}),
        json!({"occurrence": pl, "grounded": false}),
    ] {
        let grounded = run("occurrence.ground", p.clone());
        matches(output_schema_for::<GroundResult>(), "occurrence.ground", &grounded);
        assert_eq!(grounded["occurrence"], pl);
    }

    // Copy (an instance), in each form. What the handler doesn't read for the form is no problem.
    for p in [
        json!({"component": "Plate"}),
        json!({"component": plate["component"], "translate": [0, 30, 0], "parent": 0}),
        json!({"occurrence": pl, "translate": [0, 40, 0]}),
        json!({"occurrence": pl, "component": [1, 2], "parent_path": "ignored"}),
        json!({"occurrence": plate_name, "path": [pl], "translate": {"x": 0, "y": 0, "z": 5}}),
        json!({"occurrence": pl, "path": [pl], "in_place": true, "parent_path": [], "translate": [0, 0, 5]}),
        json!({"occurrence": pl, "in_place": false, "parent_path": "ignored", "parent": 0}),
    ] {
        let copy = run("occurrence.copy", p.clone());
        matches(output_schema_for::<CopyResult>(), "occurrence.copy", &copy);
        assert_eq!(copy["component"], plate["component"], "{p}: {copy}");
    }

    // Paste New.
    for p in [
        json!({"component": "Plate"}),
        json!({"component": plate["component"], "occurrence": pl, "translate": [0, 0, 60], "parent": 0}),
        json!({"component": "Plate", "occurrence": pl, "path": [pl]}),
        json!({"component": "Plate", "parent_path": "ignored"}),
        json!({"component": "Plate", "in_place": true, "parent_path": [], "translate": {"x": 5, "y": 0, "z": 0}}),
    ] {
        let pasted = run("component.paste_new", p.clone());
        matches(output_schema_for::<PasteNewResult>(), "component.paste_new", &pasted);
        assert!(pasted["bodies"].as_u64().is_some_and(|n| n >= 1) && pasted["component"] != plate["component"], "{p}: {pasted}");
    }

    // Drag.
    for p in [
        json!({"occurrence": pl, "at": [25, 5, 5], "point": [25, 15, 5]}),
        json!({"path": [pl], "at": {"x": 25, "y": 15, "z": 5}, "point": {"x": 25, "y": 5, "z": 5}, "continuous": true}),
        json!({"occurrence": pl, "path": [pl], "at": [0, 0, 0], "point": [0, 0, 0]}),
    ] {
        let dragged = run("occurrence.drag", p.clone());
        matches(output_schema_for::<DragResult>(), "occurrence.drag", &dragged);
        assert_eq!(dragged["continuous"], p.get("continuous") == Some(&json!(true)), "{p}: {dragged}");
    }

    // Occurrence Physical Properties.
    for p in [
        json!({"occurrence": pl}),
        json!({"occurrence": pl, "path": [pl], "descendants": false}),
        json!({"occurrence": pl, "about": [1, 2, 3]}),
        json!({"occurrence": plate_name, "about": {"x": 1, "y": 2, "z": 3}, "descendants": true}),
        json!({"occurrence": base_at, "about": null}),
    ] {
        let measured = run("occurrence.properties", p.clone());
        matches(output_schema_for::<OccurrencePropertiesResult>(), "occurrence.properties", &measured);
        assert!(measured["name"].is_string() && measured["total"]["volume_mm3"].as_f64().is_some_and(|v| v > 0.0), "{p}: {measured}");
        assert_eq!(measured["total"].get("inertia_about").is_some(), p.get("about").is_some_and(|a| !a.is_null()), "{p}: {measured}");
    }

    // Move Bodies to Component, and Move Sketches to Component.
    let moved = run("component.move_bodies", json!({"bodies": ["BaseBox"], "component": "Spare"}));
    assert_eq!(moved["features"].as_array().map(Vec::len), Some(1), "{moved}");
    matches(output_schema_for::<MoveBodiesResult>(), "component.move_bodies", &moved);
    let moved =
        run("component.move_bodies", json!({"bodies": ["BaseBox"], "component": "Base", "component_path": [base_at], "from_path": [spare_at]}));
    matches(output_schema_for::<MoveBodiesResult>(), "component.move_bodies", &moved);
    let moved = run("component.move_sketches", json!({"sketches": ["S"], "component": "Spare"}));
    assert_eq!(moved["component"], spare["component"], "{moved}");
    matches(output_schema_for::<MoveSketchesResult>(), "component.move_sketches", &moved);
    let moved = run("component.move_sketches", json!({"sketches": "S", "component": "Base"}));
    matches(output_schema_for::<MoveSketchesResult>(), "component.move_sketches", &moved);

    // Misspelt keys still run in Warn, say so, and the reply still matches.
    run("occurrence.move", json!({"occurrence": pl, "capture": false, "translate": [0, 2, 0]}));
    let cases: [(&str, Value, &str, schemars::Schema); 9] = [
        ("component.capture_position", json!({"captrue": true}), "captrue", output_schema_for::<CapturePositionResult>()),
        ("component.revert_position", json!({"occurence": [pl]}), "occurence", output_schema_for::<RevertPositionResult>()),
        ("occurrence.copy", json!({"component": "Plate", "translat": [1, 0, 0]}), "translat", output_schema_for::<CopyResult>()),
        ("component.paste_new", json!({"component": "Plate", "paretn": 0}), "paretn", output_schema_for::<PasteNewResult>()),
        ("occurrence.move", json!({"occurrence": pl, "translte": [1, 0, 0]}), "translte", output_schema_for::<MoveResult>()),
        (
            "occurrence.drag",
            json!({"occurrence": pl, "at": [0, 0, 0], "point": [1, 0, 0], "contiuous": true}),
            "contiuous",
            output_schema_for::<DragResult>(),
        ),
        ("occurrence.properties", json!({"occurrence": pl, "descendents": false}), "descendents", output_schema_for::<OccurrencePropertiesResult>()),
        (
            "component.move_bodies",
            json!({"bodies": ["BaseBox"], "component": "Spare", "compnent_path": [spare_at]}),
            "compnent_path",
            output_schema_for::<MoveBodiesResult>(),
        ),
        (
            "component.move_sketches",
            json!({"sketches": ["S"], "component": "Plate", "componnt": 1}),
            "componnt",
            output_schema_for::<MoveSketchesResult>(),
        ),
    ];
    for (id, p, key, schema) in cases {
        let warned = run(id, p);
        let warnings = warned["param_warnings"].as_array().cloned().unwrap_or_default();
        assert!(warnings.len() == 1 && warnings[0].as_str().is_some_and(|w| w.contains(key)), "{id}: {warned}");
        assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{id}: {warned}");
    }
    // Last, as it toggles the placement's grounding.
    let warned = run("occurrence.ground", json!({"occurrence": pl, "grouned": true}));
    let schema = output_schema_for::<GroundResult>();
    assert!(warned["param_warnings"].as_array().is_some_and(|w| w.len() == 1 && w[0].as_str().is_some_and(|w| w.contains("grouned"))), "{warned}");
    assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");
}

/// A move that joints undo replies with `reverted` and a `warning`, which the output schema
/// describes.
#[test]
fn a_move_its_joints_undo_matches_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let base = run("component.create", json!({"name": "Base"}))["occurrence"].clone();
    run("solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run("component.activate", json!({"component": 0}));
    let plate = run("component.create", json!({"name": "Plate", "parent": 0, "grounded": false}))["occurrence"].clone();
    run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    run("component.activate", json!({"component": 0}));
    run("joint.as_built", json!({"type": "rigid", "a": base, "b": plate, "name": "Mount"}));
    let moved = run("occurrence.move", json!({"occurrence": plate, "translate": [0, 0, 30]}));
    assert_eq!(moved["reverted"], true, "{moved}");
    assert!(moved["warning"].as_str().is_some_and(|w| w.contains("Mount")), "{moved}");
    matches(output_schema_for::<MoveResult>(), "occurrence.move", &moved);
}
