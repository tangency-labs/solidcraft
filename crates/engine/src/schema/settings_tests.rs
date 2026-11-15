use serde_json::{Value, json};

use super::{
    ProfileApplyParams, ProfileApplyResult, ProfileDeleteParams, ProfileDeleteResult, ProfileExportParams, ProfileExportResult, ProfileImportParams,
    ProfileImportResult, ProfileListParams, ProfileListResult, ProfileRenameParams, ProfileRenameResult, ProfileSaveParams, ProfileSaveResult,
};
use crate::Session;
use crate::cmd::settings::{MAX_PATH, MAX_PROFILE_NAME, path_value, profile_name};
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ProfileListParams>(), &[]);
    assert_keys(input_schema_for::<ProfileSaveParams>(), &["name"]);
    assert_keys(input_schema_for::<ProfileApplyParams>(), &["name"]);
    assert_keys(input_schema_for::<ProfileRenameParams>(), &["name", "to"]);
    assert_keys(input_schema_for::<ProfileDeleteParams>(), &["name"]);
    assert_keys(input_schema_for::<ProfileExportParams>(), &["name", "path"]);
    assert_keys(input_schema_for::<ProfileImportParams>(), &["name", "path", "profile"]);
}

#[test]
fn the_names_the_handler_needs_are_required() {
    for (id, schema, required) in [
        ("settings.profile.save", input_schema_for::<ProfileSaveParams>(), json!(["name"])),
        ("settings.profile.apply", input_schema_for::<ProfileApplyParams>(), json!(["name"])),
        ("settings.profile.delete", input_schema_for::<ProfileDeleteParams>(), json!(["name"])),
        ("settings.profile.export", input_schema_for::<ProfileExportParams>(), json!(["name"])),
        ("settings.profile.rename", input_schema_for::<ProfileRenameParams>(), json!(["name", "to"])),
    ] {
        assert_eq!(schema.as_value().get("required"), Some(&required), "{id}");
    }
    // Import needs a file or a document, and which one it got stays the handler's to refuse; its
    // `name` is read only when it is there, so nothing is required outright.
    let import = input_schema_for::<ProfileImportParams>();
    assert_eq!(import.as_value().get("anyOf"), Some(&json!([{"required": ["path"]}, {"required": ["profile"]}])));
    assert_eq!(import.as_value().get("required"), None);
}

#[test]
fn names_and_paths_are_read_as_the_handler_reads_them() {
    for given in [
        json!("Laptop"),
        json!("  Laptop  "),
        json!("é".repeat(MAX_PROFILE_NAME)),
        json!(""),
        json!("   "),
        json!("a\nb"),
        json!("x".repeat(MAX_PROFILE_NAME + 1)),
        json!(3),
        Value::Null,
    ] {
        let kept = given.as_str().and_then(profile_name);
        let parsed = serde_json::from_value::<ProfileSaveParams>(json!({ "name": given.clone() }));
        assert_eq!(parsed.is_ok(), kept.is_some(), "{given}");
        if let Some(kept) = kept {
            assert_eq!(parsed.unwrap().name.map(|name| name.0).as_deref(), Some(kept), "{given}");
        }
    }
    for given in [json!("/tmp/p.json"), json!("profile.json"), json!(""), json!("  "), json!("x".repeat(MAX_PATH + 1)), json!(3), Value::Null] {
        let kept = path_value(&given);
        let parsed = serde_json::from_value::<ProfileExportParams>(json!({ "name": "A", "path": given.clone() }));
        assert_eq!(parsed.is_ok(), kept.is_some() || given.is_null(), "{given}");
        if let Some(kept) = kept {
            assert_eq!(parsed.unwrap().path.map(|path| path.0).as_deref(), Some(kept), "{given}");
        }
    }
}

#[test]
fn the_published_schemas_describe_what_the_readers_keep() {
    let root = input_schema_for::<ProfileExportParams>();
    let root = root.as_value();
    let (name, path) = (resolved(root, "name"), resolved(root, "path"));
    // JSON Schema can't say "no control characters" or count bytes, so the description says it.
    // The `name` these commands need is required and refuses null; `path` reads null as left out.
    for (property, bound, types) in [(name, MAX_PROFILE_NAME, json!("string")), (path, MAX_PATH, json!(["string", "null"]))] {
        assert_eq!(property["type"], types, "{property}");
        assert!(
            property["description"].as_str().unwrap_or_default().contains(&bound.to_string()),
            "the bound belongs in the description: {property}"
        );
    }
    let name = &root["properties"]["name"];
    let path = &root["properties"]["path"];
    for good in ["Laptop", "  Laptop  ", &"é".repeat(MAX_PROFILE_NAME)] {
        assert!(conforms(root, name, &json!(good)), "{good}");
    }
    for good in ["/tmp/p.json", "C:\\tmp\\p.json"] {
        assert!(conforms(root, path, &json!(good)), "{good}");
    }
    // A `null` the handler reads as the key being left out stays valid in the published schema; a
    // name the command needs does not.
    assert!(conforms(root, path, &Value::Null), "export takes a null path");
    assert!(!conforms(root, name, &Value::Null), "export needs its name");
    let import = input_schema_for::<ProfileImportParams>();
    let import = import.as_value();
    let profile = &import["properties"]["profile"];
    assert_eq!(resolved(import, "profile")["type"], json!(["object", "null"]), "{profile}");
    assert!(conforms(import, profile, &json!({ "format": "solidcraft-profile/1" })));
    assert!(conforms(import, profile, &Value::Null));
    assert!(!conforms(import, profile, &json!("text")));
    // Import reads a null `path` or `name` as left out too, and its `name` is never needed.
    for key in ["path", "name"] {
        let property = resolved(import, key);
        assert_eq!(property["type"], json!(["string", "null"]), "{key}: {property}");
        assert!(conforms(import, &import["properties"][key], &Value::Null), "import takes a null {key}");
    }
}

/// The schema a key's value must match, following the `$ref` the contract publishes for it.
fn resolved<'a>(root: &'a Value, key: &str) -> &'a Value {
    let property = &root["properties"][key];
    match property.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/$defs/")) {
        Some(name) => &root["$defs"][name],
        None => property,
    }
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("settings.profile.save", json!({})),
        ("settings.profile.save", json!({"name": null})),
        ("settings.profile.save", json!({"name": "   "})),
        ("settings.profile.save", json!({"name": 3})),
        ("settings.profile.save", json!({"name": "x".repeat(MAX_PROFILE_NAME + 1)})),
        ("settings.profile.apply", json!({"name": "a\nb"})),
        ("settings.profile.rename", json!({"name": "A"})),
        ("settings.profile.rename", json!({"name": "A", "to": ""})),
        ("settings.profile.delete", json!({"name": 3})),
        ("settings.profile.export", json!({"name": "A", "path": "x".repeat(MAX_PATH + 1)})),
        ("settings.profile.export", json!({"name": "A", "path": "  "})),
        ("settings.profile.import", json!({})),
        ("settings.profile.import", json!({"profile": "text"})),
        ("settings.profile.import", json!({"path": "/tmp/p.json", "profile": {"format": "solidcraft-profile/1"}})),
        ("settings.profile.import", json!({"path": "", "name": "A"})),
        ("settings.profile.import", json!({"path": "/tmp/p.json", "name": ""})),
    ]);
}

/// A `null` the handler reads as the key being left out stays accepted in strict, as the published
/// schema now says: a call the handler takes must not be refused by the check.
#[test]
fn strict_takes_the_nulls_the_handler_takes() {
    let doc = json!({ "format": "solidcraft-profile/1", "name": "Laptop" });
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    for (id, p) in [
        ("settings.profile.export", json!({"name": "A", "path": null})),
        ("settings.profile.import", json!({"path": null, "profile": doc})),
        ("settings.profile.import", json!({"profile": doc, "name": null})),
    ] {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    }
}

#[test]
fn replies_match_the_output_schemas() {
    let doc = json!({ "format": "solidcraft-profile/1", "name": "Laptop" });
    let cases: Vec<(&str, Value, Vec<Value>)> = vec![
        ("settings.profile.list", output_for::<ProfileListResult>(), vec![json!({})]),
        ("settings.profile.save", output_for::<ProfileSaveResult>(), vec![json!({"name": "  Laptop "})]),
        ("settings.profile.apply", output_for::<ProfileApplyResult>(), vec![json!({"name": "Laptop"})]),
        ("settings.profile.rename", output_for::<ProfileRenameResult>(), vec![json!({"name": "A", "to": "B"})]),
        ("settings.profile.delete", output_for::<ProfileDeleteResult>(), vec![json!({"name": "A"})]),
        (
            "settings.profile.export",
            output_for::<ProfileExportResult>(),
            vec![json!({"name": "A"}), json!({"name": "A", "path": "/tmp/p.json"}), json!({"name": "A", "path": null})],
        ),
        (
            "settings.profile.import",
            output_for::<ProfileImportResult>(),
            vec![json!({"path": "/tmp/p.json"}), json!({"profile": doc, "name": "B"}), json!({"profile": doc, "name": null})],
        ),
    ];
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    for (id, output, calls) in cases {
        for p in &calls {
            let reply = s.execute(id, p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            assert!(reply.get("param_warnings").is_none(), "{id} {p}: {reply}");
            assert!(conforms(&output, &output, &reply), "{id} {p}: {reply} doesn't match {output}");
        }
        // A key the command doesn't take is a warning, and the reply keeps its shape.
        let mut hostile = calls[0].clone();
        hostile["unknown_key"] = json!(1);
        let reply = s.execute(id, &hostile).unwrap_or_else(|e| panic!("{id} {hostile}: {e}"));
        assert!(reply.get("param_warnings").is_some(), "{id}: {reply}");
        assert!(conforms(&output, &output, &reply), "{id}: {reply} doesn't match {output}");
        s.param_check = ParamCheck::Strict;
        assert!(s.execute(id, &hostile).is_err(), "{id} in strict");
        s.param_check = ParamCheck::Warn;
    }
}

fn output_for<T: schemars::JsonSchema>() -> Value {
    output_schema_for::<T>().as_value().clone()
}
