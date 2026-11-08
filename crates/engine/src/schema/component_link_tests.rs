use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use solidcraft_doc::DesignLink;

use super::{
    BreakLinkParams, BreakLinkResult, CopiesParams, CopiesResult, ExtractParams, ExtractResult, InsertDesignParams, InsertDesignResult, LinksParams,
    LinksResult, RelinkParams, RelinkResult, ReplaceWithLinkParams, ReplaceWithLinkResult, UpdateLinksParams, UpdateLinksResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<InsertDesignParams>(), &["name", "parent", "path", "reuse", "translate"]);
    assert_keys(input_schema_for::<UpdateLinksParams>(), &["component", "components"]);
    assert_keys(input_schema_for::<RelinkParams>(), &["component", "path"]);
    assert_keys(input_schema_for::<BreakLinkParams>(), &["component"]);
    assert_keys(input_schema_for::<LinksParams>(), &[]);
    assert_keys(input_schema_for::<ExtractParams>(), &["component", "path"]);
    assert_keys(input_schema_for::<CopiesParams>(), &["component"]);
    assert_keys(input_schema_for::<ReplaceWithLinkParams>(), &["component", "components", "path", "with"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("component.insert_design", json!({})),
        ("component.insert_design", json!({"path": ""})),
        ("component.insert_design", json!({"path": "  "})),
        ("component.insert_design", json!({"path": 5})),
        ("component.insert_design", json!({"path": null})),
        ("component.insert_design", json!({"path": "x".repeat(4096)})),
        ("component.insert_design", json!({"path": "plate.solidcraft", "translate": [1, 2]})),
        ("component.insert_design", json!({"path": "plate.solidcraft", "translate": null})),
        ("component.insert_design", json!({"path": "plate.solidcraft", "translate": [1e9, 0, 0]})),
        ("component.update_links", json!({"component": 0, "components": [0]})),
        ("component.update_links", json!({"component": true})),
        ("component.update_links", json!({"component": null})),
        ("component.update_links", json!({"components": "Plate"})),
        ("component.update_links", json!({"components": []})),
        ("component.update_links", json!({"components": [true]})),
        ("component.update_links", json!({"component": "Nope"})),
        ("component.relink", json!({})),
        ("component.relink", json!({"component": null, "path": "plate.solidcraft"})),
        ("component.relink", json!({"component": [1], "path": "plate.solidcraft"})),
        ("component.break_link", json!({})),
        ("component.break_link", json!({"component": [1]})),
        ("component.extract", json!({})),
        ("component.extract", json!({"component": 0})),
        ("component.extract", json!({"component": true, "path": "plate.solidcraft"})),
        ("component.copies", json!({})),
        ("component.copies", json!({"component": false})),
        ("component.replace_with_link", json!({})),
        ("component.replace_with_link", json!({"component": true})),
        ("component.replace_with_link", json!({"components": [true]})),
        ("component.replace_with_link", json!({"component": 0})),
        ("component.replace_with_link", json!({"component": 0, "with": true})),
        ("component.replace_with_link", json!({"component": 0, "path": ""})),
        ("component.replace_with_link", json!({"component": 0, "path": 7})),
    ]);
}

/// `path` is not read when `with` is given, so a bad one is no problem for the check either.
#[test]
fn replace_with_link_ignores_path_when_with_is_given() {
    let issues = |p: Value| crate::schema::issues("component.replace_with_link", &p).len();
    assert_eq!(issues(json!({"component": 1, "with": "Plate", "path": ""})), 0);
    assert_eq!(issues(json!({"component": 1, "path": ""})), 1);
}

/// The contracts say what the handlers need: the keys a handler refuses without are required in
/// the schemas, though the types leave them to the handlers' messages, and a component is an id
/// or a name.
#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<InsertDesignParams>(),
        &[
            (json!({"path": "plate.solidcraft"}), true),
            (json!({"path": "plate.solidcraft", "name": "P", "parent": "Arm", "translate": [1, 2, 3], "reuse": true}), true),
            (json!({"path": "plate.solidcraft", "parent": 4, "translate": {"x": 1, "y": 2, "z": 3}}), true),
            (json!({}), false),
            (json!({"path": "plate.solidcraft", "parent": true}), false),
            (json!({"path": "plate.solidcraft", "translate": [1, 2]}), false),
        ],
    );
    takes(
        input_schema_for::<UpdateLinksParams>(),
        &[
            (json!({}), true),
            (json!({"component": "Plate"}), true),
            (json!({"components": ["Plate", 3]}), true),
            (json!({"component": 1, "components": [1]}), false),
            (json!({"components": []}), false),
            (json!({"components": [true]}), false),
        ],
    );
    takes(
        input_schema_for::<RelinkParams>(),
        &[
            (json!({"component": "Plate", "path": "b.solidcraft"}), true),
            (json!({"component": "Plate"}), false),
            (json!({"path": "b.solidcraft"}), false),
        ],
    );
    takes(input_schema_for::<BreakLinkParams>(), &[(json!({"component": 2}), true), (json!({}), false), (json!({"component": null}), false)]);
    takes(
        input_schema_for::<ExtractParams>(),
        &[(json!({"component": "Plate", "path": "plate.solidcraft"}), true), (json!({"component": "Plate"}), false), (json!({"path": "p"}), false)],
    );
    takes(input_schema_for::<CopiesParams>(), &[(json!({"component": 2}), true), (json!({}), false)]);
    takes(
        input_schema_for::<ReplaceWithLinkParams>(),
        &[
            (json!({"component": "Copy", "with": "Plate"}), true),
            (json!({"components": ["A", 2], "path": "plate.solidcraft"}), true),
            (json!({"with": "Plate"}), false),
            (json!({"component": "Copy"}), false),
            (json!({"component": "Copy", "with": true}), false),
        ],
    );
}

/// The warning for what a key gets wrong, and the check that a list of components takes only
/// what the handlers read.
#[test]
fn the_check_names_what_is_wrong_with_a_key() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(issues("component.insert_design", json!({"path": "p.solidcraft", "translate": [1]})), ["`translate` must be [x, y, z] in mm"]);
    assert_eq!(issues("component.insert_design", json!({"path": "p.solidcraft", "parent": true})), ["`parent` must be a component id or name"]);
    assert_eq!(issues("component.update_links", json!({"component": 1, "components": [1]})), ["give `component` or `components`, not both"]);
    assert_eq!(issues("component.update_links", json!({"components": [true]})), ["`component` must be a component id or name"]);
    assert_eq!(issues("component.replace_with_link", json!({"component": 1})), Vec::<String>::new());
    assert_eq!(issues("component.replace_with_link", json!({"component": 1, "path": 3})), ["`with` or `path` is required"]);
    assert_eq!(issues("component.relink", json!({"component": 1, "path": "b.solidcraft", "paths": 1})).len(), 1);
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-link-schema-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

fn save_box(path: &Path, height: u32) {
    let mut part = Session::default();
    for (id, p) in [("solid.box", json!({"length": 10, "width": 20, "height": height, "body_name": "Plate"})), ("file.save", json!({"path": path}))] {
        part.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
}

fn matches(schema: schemars::Schema, id: &str, reply: &Value) {
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// Insert (new, reused, in a parent, placed), list, update (with a design that can't be read
/// named in `failed`), relink and break, in Warn: each real reply matches its output schema and
/// carries no warning. A list the network or a device refuses has the other shapes of an entry.
#[test]
fn link_replies_match_the_output_schema() {
    let dir = folder("links");
    let (a, b, c) = (dir.join("a.solidcraft"), dir.join("b.solidcraft"), dir.join("c.solidcraft"));
    for (file, height) in [(&a, 10), (&b, 20), (&c, 30)] {
        save_box(file, height);
    }
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let first = run("component.insert_design", json!({"path": a, "name": "A"}));
    matches(output_schema_for::<InsertDesignResult>(), "component.insert_design", &first);
    assert!(first["created_bodies"] == first["bodies"] && first["bodies"] == json!(["Plate"]), "{first}");
    let reused = run("component.insert_design", json!({"path": a, "reuse": true, "translate": [50, 0, 0]}));
    assert!(reused.get("name").is_none() && reused["component"] == first["component"], "{reused}");
    matches(output_schema_for::<InsertDesignResult>(), "component.insert_design", &reused);
    let second = run("component.insert_design", json!({"path": b, "name": "B", "parent": first["component"], "translate": {"x": 1, "y": 2, "z": 3}}));
    matches(output_schema_for::<InsertDesignResult>(), "component.insert_design", &second);
    let third = run("component.insert_design", json!({"path": c, "name": "C"}));
    matches(output_schema_for::<InsertDesignResult>(), "component.insert_design", &third);

    let listed = run("component.links", json!({}));
    assert_eq!(listed["links"].as_array().map(Vec::len), Some(3), "{listed}");
    assert!(listed["links"][0]["found"] == true && listed["links"][0]["changed"] == false, "{listed}");
    matches(output_schema_for::<LinksResult>(), "component.links", &listed);

    // The design B links changed, and C's file is gone: B updates, C is named in `failed`.
    save_box(&b, 25);
    std::fs::remove_file(&c).unwrap_or_else(|e| panic!("{e}"));
    let update = run("component.update_links", json!({"components": ["B", "C", "A"]}));
    assert!(update["updated"] == json!(["B"]) && update["unchanged"] == json!(["A"]), "{update}");
    assert!(update["failed"][0]["name"] == "C" && update["failed"][0]["component"] == third["component"], "{update}");
    matches(output_schema_for::<UpdateLinksResult>(), "component.update_links", &update);
    save_box(&b, 30);
    let all = run("component.update_links", json!({"component": "B"}));
    assert_eq!(all["updated"], json!(["B"]), "{all}");
    assert_eq!(all["failed"], json!([]), "{all}");
    matches(output_schema_for::<UpdateLinksResult>(), "component.update_links", &all);
    let nothing = run("component.update_links", json!({"component": "A"}));
    assert_eq!(nothing["unchanged"], json!(["A"]), "{nothing}");
    matches(output_schema_for::<UpdateLinksResult>(), "component.update_links", &nothing);

    let moved = run("component.relink", json!({"component": "A", "path": b}));
    assert!(moved["geometry_changed"] == true && moved["joint_errors"] == json!([]), "{moved}");
    matches(output_schema_for::<RelinkResult>(), "component.relink", &moved);
    let broken = run("component.break_link", json!({"component": "A"}));
    assert_eq!(broken["component"], first["component"], "{broken}");
    matches(output_schema_for::<BreakLinkResult>(), "component.break_link", &broken);

    // A misspelt key still runs in Warn, and says so, with a reply that matches.
    let warned = run("component.links", json!({"lnks": true}));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    let schema = output_schema_for::<LinksResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");

    // Links the network or a device refuses are not looked for, and say why.
    let (feature, component) =
        (s.doc.links[&third["component"].as_u64().unwrap_or_default()].feature, third["component"].as_u64().unwrap_or_default());
    for (link, host) in [(r"\\evil.example\share\c.solidcraft", true), (r"\\.\COM1", false)] {
        s.doc_mut().links.insert(component, DesignLink { path: link.to_string(), feature, ..Default::default() });
        let listed = s.execute("component.links", &json!({})).unwrap_or_else(|e| panic!("{e}"));
        let entry = listed["links"].as_array().and_then(|l| l.iter().find(|e| e["component"] == third["component"])).cloned().unwrap_or_default();
        assert!(entry["found"].is_null() && entry["reason"].is_string() && entry.get("changed").is_none(), "{listed}");
        assert_eq!(entry.get("host").is_some(), host, "{listed}");
        matches(output_schema_for::<LinksResult>(), "component.links", &listed);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Extract a component to a file, find its identical copy, and replace the copy with a
/// placement of the link, in Warn: each real reply matches its output schema without warnings.
#[test]
fn extract_copies_and_replace_replies_match_the_output_schema() {
    let dir = folder("extract");
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let made = run("component.create", json!({"name": "Plate", "grounded": false}));
    let (plate, occurrence) = (made["component"].clone(), made["occurrence"].clone());
    run("solid.box", json!({"length": 40, "width": 30, "height": 12, "body_name": "PlateBody"}));
    run("component.activate", json!({"component": 0}));
    let copy = run("component.paste_new", json!({"component": plate, "occurrence": occurrence, "translate": [0, 80, 0]}));
    let file = dir.join("plate.solidcraft");
    let extracted = run("component.extract", json!({"component": "Plate", "path": file}));
    assert_eq!(extracted["bodies"], json!(["PlateBody"]), "{extracted}");
    assert_eq!(extracted["copies"].as_array().map(Vec::len), Some(1), "{extracted}");
    assert_eq!(extracted["copies"][0]["component"], copy["component"], "{extracted}");
    matches(output_schema_for::<ExtractResult>(), "component.extract", &extracted);
    let found = run("component.copies", json!({"component": plate}));
    assert_eq!(found["copies"], extracted["copies"], "{found}");
    matches(output_schema_for::<CopiesResult>(), "component.copies", &found);
    let replaced = run("component.replace_with_link", json!({"components": [copy["component"]], "with": plate}));
    assert!(replaced["component"] == plate && replaced["replaced"].as_array().is_some_and(|r| r.len() == 1), "{replaced}");
    matches(output_schema_for::<ReplaceWithLinkResult>(), "component.replace_with_link", &replaced);
    assert_eq!(run("component.copies", json!({"component": plate}))["copies"], json!([]));
    let _ = std::fs::remove_dir_all(&dir);
}
