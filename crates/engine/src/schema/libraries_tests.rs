use serde_json::{Value, json};

use super::{LibraryApplied, LibraryApplyParams, LibraryExportParams, LibraryExported, LibraryInspectParams, LibraryInspected};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A library file in the temporary folder, removed when dropped.
struct File(String);

impl File {
    fn new(name: &str) -> Self {
        let name = format!("solidcraft-library-schema-{}-{name}.json", std::process::id());
        File(solidcraft_io::vfs::temp_dir().join(name).to_string_lossy().into_owned())
    }
}

impl Drop for File {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<LibraryInspectParams>(), &["path"]);
    assert_keys(input_schema_for::<LibraryExportParams>(), &["format", "id", "name", "overwrite", "path", "provenance"]);
    assert_keys(input_schema_for::<LibraryApplyParams>(), &["entry", "name", "path", "revision", "targets"]);
}

#[test]
fn strict_keeps_todays_errors() {
    let file = File::new("strict");
    assert_strict_keeps_todays_errors(&[
        ("library.inspect", json!({"path": ""})),
        ("library.inspect", json!({"path": file.0})),
        ("library.export", json!({"path": file.0.replace(".json", ".txt")})),
        ("library.export", json!({"path": file.0, "format": "solidcraft-library/3"})),
        ("library.export", json!({"path": file.0, "id": "Not An Id"})),
        ("library.export", json!({"path": file.0, "provenance": {"license": ""}})),
        ("library.apply", json!({"path": file.0, "entry": "user.local/appearance-0", "revision": "0"})),
        ("library.apply", json!({"path": file.0, "entry": "e", "revision": "0", "targets": {"faces": [{"body": "B", "point": [1e10, 0, 0]}]}})),
    ]);
}

/// #10938: exporting, reading and applying a library reply as their output schemas say, in Warn
/// without warnings; an appearance entry and a physical material entry each take their own
/// branch of the `result` schema. Like the handlers, warn and strict refuse an unknown key.
#[test]
fn library_commands_reply_with_their_schemas() {
    let file = File::new("replies");
    let (inspected, exported, applied) =
        (output_schema_for::<LibraryInspected>(), output_schema_for::<LibraryExported>(), output_schema_for::<LibraryApplied>());
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: &schemars::Schema, r: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
    };
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "appearance.assign", json!({"bodies": ["Body1"], "color": [180, 120, 70], "opacity": 0.8}));
    run(&mut s, "material.create", json!({"name": "Timber", "density_g_cm3": 0.7, "color": "#b47846"}));
    run(&mut s, "material.assign", json!({"bodies": ["Body1"], "material": "Timber"}));
    let provenance = json!({"license": "CC0-1.0", "author": "A tester", "source_url": "https://example.com/library"});
    let r = run(&mut s, "library.export", json!({"path": file.0, "id": "user.test", "name": "Test", "provenance": provenance}));
    matches(&exported, &r);
    assert!(r["entries"].as_u64() >= Some(2), "{r}");
    let v1 = File::new("replies-v1");
    matches(&exported, &run(&mut s, "library.export", json!({"path": v1.0, "format": "solidcraft-library/1", "overwrite": true})));
    let r = run(&mut s, "library.inspect", json!({"path": file.0}));
    matches(&inspected, &r);
    let entries = r["catalog"]["entries"].as_array().cloned().unwrap_or_default();
    let entry = |kind: &str| {
        entries
            .iter()
            .find(|e| e["definition"]["kind"] == kind)
            .map(|e| (e["id"].clone(), e["revision"].clone()))
            .unwrap_or_else(|| panic!("no {kind} in {r}"))
    };
    let (look, look_revision) = entry("appearance");
    let targets = json!({"bodies": ["Body1"], "faces": [{"body": "Body1", "point": [10, 5, 5]}], "components": []});
    let r = run(&mut s, "library.apply", json!({"path": file.0, "entry": look, "revision": look_revision, "targets": targets}));
    matches(&applied, &r);
    assert!(r["result"]["appearance"].is_object(), "{r}");
    let (material, material_revision) = entry("material");
    let r = run(
        &mut s,
        "library.apply",
        json!({"path": file.0, "entry": material, "revision": material_revision, "targets": {"bodies": ["Body1"]}, "name": null}),
    );
    matches(&applied, &r);
    assert_eq!((r["result"]["material"].as_str(), r["result"]["reused"].as_bool()), (Some("Timber"), Some(true)), "{r}");
    for level in [ParamCheck::Warn, ParamCheck::Strict] {
        let mut s = Session { param_check: level, ..Session::default() };
        let error = s.execute("library.inspect", &json!({"path": file.0, "pth": 1})).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("pth"), "{level:?}: {error}");
    }
}
