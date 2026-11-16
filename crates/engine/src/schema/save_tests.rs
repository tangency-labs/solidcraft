use std::path::PathBuf;

use serde_json::{Value, json};

use super::{SaveAsParams, SaveParams, SaveResult};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    let keys = ["cache", "compression", "format", "overwrite", "path", "preview", "sign", "signing_key"];
    assert_keys(input_schema_for::<SaveParams>(), &keys);
    assert_keys(input_schema_for::<SaveAsParams>(), &keys);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("file.save", json!({})),
        ("file.save", json!({"path": ""})),
        ("file.save", json!({"path": "  "})),
        ("file.save", json!({"path": null})),
        ("file.save_as", json!({})),
        ("file.save_as", json!({"path": ""})),
        ("file.save_as", json!({"path": "  "})),
        ("file.save_as", json!({"path": 5})),
        ("file.save_as", json!({"path": null})),
        ("file.save_as", json!({"path": ["a.sc"]})),
        ("file.save_as", json!({"path": "x".repeat(4096)})),
        // Words the handler refuses itself, in its own wording (which names `file.save`).
        ("file.save_as", json!({"path": "/tmp/solidcraft-save-schema-never.sc", "format": "xml"})),
        ("file.save_as", json!({"path": "/tmp/solidcraft-save-schema-never.sc", "compression": "max"})),
        ("file.save", json!({"path": "/tmp/solidcraft-save-schema-never.sc", "format": "xml"})),
        ("file.save", json!({"path": "/tmp/solidcraft-save-schema-never.sc", "compression": "max"})),
        // A name that opens as another kind of file.
        ("file.save_as", json!({"path": "/tmp/solidcraft-save-schema-never.step"})),
        ("file.save", json!({"path": "/tmp/solidcraft-save-schema-never.stl"})),
    ]);
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_path_to_error::deserialize::<_, T>(&v).map_err(|e| e.to_string())
}

/// The types read each key as the handlers do: `null` is absent, `file.save` takes any text for
/// `path` (blank means the current file) and `file.save_as` takes not-blank text under 4096
/// bytes, with the handler's message for the rest.
#[test]
fn the_types_read_each_key_as_the_handlers_do() {
    let all = json!({"format": "json", "compression": "compact", "cache": false, "preview": true, "overwrite": true});
    for key in ["path", "format", "compression", "cache", "preview", "overwrite"] {
        assert!(parse::<SaveParams>(json!({ key: null })).is_ok(), "{key}: null reads as left out");
    }
    assert!(parse::<SaveParams>(json!({})).is_ok());
    assert!(parse::<SaveParams>(json!({"path": ""})).is_ok());
    assert!(parse::<SaveParams>(json!({"path": "a.sc"})).is_ok());
    let mut with_path = all.clone();
    with_path["path"] = json!("a.sc");
    assert!(parse::<SaveParams>(with_path.clone()).is_ok());
    assert!(parse::<SaveAsParams>(with_path).is_ok());
    assert!(parse::<SaveParams>(json!({"path": 5})).is_err());
    assert!(parse::<SaveParams>(json!({"cache": "yes"})).is_err());

    assert!(parse::<SaveAsParams>(json!({})).is_ok(), "a missing path is the handler's to refuse");
    assert!(parse::<SaveAsParams>(json!({"path": "x".repeat(4095)})).is_ok());
    for bad in [json!(""), json!(" \t"), json!(7), json!(null), json!(["a"]), json!("x".repeat(4096))] {
        assert_eq!(parse::<SaveAsParams>(json!({"path": bad})).err().as_deref().map(|e| e.ends_with("`path` is required")), Some(true), "{bad}");
    }
    // The words the handler checks are left to it, in both commands.
    assert!(parse::<SaveAsParams>(json!({"path": "a.sc", "format": "xml", "compression": "max"})).is_ok());
}

#[test]
fn the_schemas_take_what_the_handlers_read() {
    let takes = |schema: schemars::Schema, cases: &[(Value, bool)]| {
        let schema = schema.as_value();
        for (p, takes) in cases {
            assert_eq!(conforms(schema, schema, p), *takes, "{p}");
        }
    };
    takes(
        input_schema_for::<SaveParams>(),
        &[
            (json!({}), true),
            (json!({"path": "a.sc", "format": "container", "compression": "fast", "cache": true, "preview": false, "overwrite": false}), true),
            (json!({"format": "json", "compression": "compact"}), true),
            (json!({"format": "xml"}), false),
            (json!({"compression": "max"}), false),
            (json!({"cache": 1}), false),
            (json!({"path": 1}), false),
        ],
    );
    takes(
        input_schema_for::<SaveAsParams>(),
        &[
            (json!({"path": "a.sc"}), true),
            (json!({"path": "a.json", "format": "json", "compression": "compact", "cache": false, "preview": true, "overwrite": true}), true),
            (json!({}), false),
            (json!({"path": "a.sc", "format": "xml"}), false),
        ],
    );
}

/// The `path` pattern is the not-blank one (`conforms` doesn't read patterns), with the length cap.
#[test]
fn save_as_path_schema_is_pinned() {
    let schema = input_schema_for::<SaveAsParams>();
    let path = &schema.as_value()["properties"]["path"];
    assert_eq!(path["pattern"], json!(crate::schema::types::not_blank_pattern()));
    assert_eq!(path["maxLength"], json!(4095));
    assert_eq!(schema.as_value()["required"], json!(["path"]));
    assert!(input_schema_for::<SaveParams>().as_value().get("required").is_none());
}

/// A misspelt key is reported by the check, with the key it probably meant.
#[test]
fn the_check_names_what_is_wrong() {
    let issues = |id: &str, p: Value| crate::schema::issues(id, &p).into_iter().map(|i| i.message).collect::<Vec<_>>();
    assert_eq!(issues("file.save", json!({"overwite": true})), ["unknown key `overwite`; did you mean `overwrite`?"]);
    assert_eq!(issues("file.save_as", json!({"path": "a.sc", "cache": "no"})), ["`cache` must be true or false"]);
    assert_eq!(issues("file.save_as", json!({"path": " "})), ["`path` is required"]);
    assert_eq!(issues("file.save", json!({"path": "a.sc", "format": "container"})), Vec::<String>::new());
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-save-schema-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    dir
}

fn matches(id: &str, reply: &Value) {
    let schema = output_schema_for::<SaveResult>();
    let schema = schema.as_value();
    assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    assert!(reply.get("param_warnings").is_none(), "{id}: {reply}");
}

/// Every form of both commands, in Warn: no `param_warnings`, and each reply matches the output
/// schema and is what the handler returned before (the keys a plain save has, nothing else).
#[test]
fn save_replies_match_the_output_schema() {
    let dir = folder("replies");
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run("solid.box", json!({"length": 10, "width": 20, "height": 5}));
    let design = dir.join("part.sc");
    let first = run("file.save_as", json!({"path": design}));
    assert_eq!(first["path"], json!(design), "{first}");
    assert!(first["bytes"].as_u64().is_some_and(|b| b > 0) && first["name"] == "part", "{first}");
    let mut keys: Vec<_> = first.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default();
    keys.sort();
    assert_eq!(keys, ["bytes", "name", "path"], "{first}");
    matches("file.save_as", &first);
    matches("file.save", &run("file.save", json!({})));
    matches("file.save", &run("file.save", json!({"path": null, "format": null})));
    matches("file.save", &run("file.save", json!({"path": ""})));
    matches("file.save", &run("file.save", json!({"path": design, "compression": "compact", "cache": false, "preview": true, "overwrite": true})));
    matches("file.save", &run("file.save", json!({"path": design, "format": "container", "compression": "fast"})));
    let json_file = dir.join("part.json");
    matches("file.save_as", &run("file.save_as", json!({"path": json_file})));
    matches("file.save_as", &run("file.save_as", json!({"path": dir.join("other.sc"), "format": "json", "overwrite": false})));
    matches("file.save", &run("file.save", json!({"path": dir.join("again.sc"), "preview": false, "cache": true})));

    // A misspelt key still runs in Warn, says so, and the reply still matches the schema.
    let warned = run("file.save", json!({"path": design, "overwite": true}));
    let warnings = warned["param_warnings"].as_array().cloned().unwrap_or_default();
    assert_eq!(warnings.len(), 1, "{warned}");
    assert!(warnings[0].as_str().is_some_and(|w| w.contains("overwite") && w.contains("overwrite")), "{warned}");
    let schema = output_schema_for::<SaveResult>();
    assert!(conforms(schema.as_value(), schema.as_value(), &warned), "{warned}");
    let warned = run("file.save_as", json!({"path": design, "compresion": "fast"}));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| w.len() == 1), "{warned}");
    // In Strict the same misspelling is refused.
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    assert!(strict.execute("file.save_as", &json!({"path": design, "overwite": true})).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file a newer SolidCraft saved keeps its format number: the reply carries it and matches. What
/// could not be kept (`warnings`, `not_kept`) is part of the type, and serializes as the handler
/// has always written it.
#[test]
fn a_newer_files_reply_matches_the_output_schema() {
    let dir = folder("newer");
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    run(&mut s, "solid.box", json!({"length": 10, "width": 20, "height": 5}));
    let plain = dir.join("plain.json");
    run(&mut s, "file.save_as", json!({"path": plain}));
    let mut file: Value = serde_json::from_str(&std::fs::read_to_string(&plain).unwrap_or_default()).unwrap_or_default();
    file["format"] = json!(solidcraft_doc::format::format_string(solidcraft_doc::format::FORMAT + 1));
    file["future_section"] = json!({"anything": [1, 2, 3]});
    file["features"][0]["draft_v2"] = json!({"angle": 3.0});
    let newer = dir.join("newer.json");
    std::fs::write(&newer, file.to_string()).unwrap_or_else(|e| panic!("{e}"));
    run(&mut s, "doc.open", json!({"path": newer}));
    let saved = run(&mut s, "file.save", json!({}));
    assert_eq!(saved["kept_format"], file["format"], "{saved}");
    assert!(saved.get("warnings").is_none() && saved.get("not_kept").is_none(), "{saved}");
    matches("file.save", &saved);
    let lost = serde_json::to_value(SaveResult {
        path: "a.sc".into(),
        bytes: 3,
        name: "a".into(),
        kept_format: Some("solidcraft/99".into()),
        warnings: Some(vec!["Not kept: x. The previous file is `a.sc.bak`.".into()]),
        not_kept: Some(vec!["x".into()]),
        param_warnings: Vec::new(),
        signature: None,
    })
    .unwrap_or_default();
    assert_eq!(
        lost,
        json!({"path": "a.sc", "bytes": 3, "name": "a", "kept_format": "solidcraft/99", "warnings": ["Not kept: x. The previous file is `a.sc.bak`."], "not_kept": ["x"]})
    );
    matches("file.save", &lost);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_script_or_agent_cannot_sign_a_save() {
    let mut s = Session::default();
    let path = std::env::temp_dir().join(format!("solidcraft-script-sign-{}.sc", std::process::id()));
    let error = s.execute("file.save", &json!({"path": path, "sign": true})).expect_err("signing needs the app's Save");
    assert!(error.to_string().contains("only available from the app's Save"), "{error}");
}
