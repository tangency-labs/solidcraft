use serde_json::{Value, json};

use super::{
    MacroCleared, MacroDeleteParams, MacroListed, MacroNoParams, MacroOpenParams, MacroOpened, MacroPlayParams, MacroPlayed, MacroRecordParams,
    MacroSaveParams, MacroSaved, MacroState, MacroStopped,
};
use crate::Session;
use crate::schema::ParamCheck;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<MacroNoParams>(), &[]);
    assert_keys(input_schema_for::<MacroRecordParams>(), &["name"]);
    assert_keys(input_schema_for::<MacroPlayParams>(), &["name", "params", "path", "source"]);
    assert_keys(input_schema_for::<MacroOpenParams>(), &["path"]);
    assert_keys(input_schema_for::<MacroSaveParams>(), &["name", "path"]);
    assert_keys(input_schema_for::<MacroDeleteParams>(), &["name"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        // Nothing is being recorded: the enabled gate refuses these the same under strict.
        ("macro.pause", json!({})),
        ("macro.stop", json!({})),
        ("macro.clear", json!({})),
        // The handler's own refusals, which the contracts leave it to make.
        ("macro.play", json!({ "source": "run(\"no.such_command\")" })),
        ("macro.save", json!({ "name": "m.rhai", "path": "/tmp/m.rhai" })),
        ("macro.open", json!({ "path": "notes.txt" })),
        ("macro.delete", json!({ "name": "../escape.rhai" })),
    ]);
}

/// The recorder's replies match their output schemas, in Warn without warnings for good calls.
#[test]
fn macro_replies_match_their_schemas() {
    let (state, stopped) = (output_schema_for::<MacroState>(), output_schema_for::<MacroStopped>());
    let (played, cleared) = (output_schema_for::<MacroPlayed>(), output_schema_for::<MacroCleared>());
    let (listed, opened) = (output_schema_for::<MacroListed>(), output_schema_for::<MacroOpened>());
    let saved = output_schema_for::<MacroSaved>();
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };

    let r = run(&mut s, "macro.record", json!({ "name": "Demo" }));
    matches(&state, &r, false);
    run(&mut s, "solid.box", json!({ "length": 40, "width": 30, "height": 10 }));
    matches(&state, &run(&mut s, "macro.pause", json!({})), false);
    matches(&state, &run(&mut s, "macro.resume", json!({})), false);
    let r = run(&mut s, "macro.stop", json!({}));
    matches(&stopped, &r, false);
    assert_eq!(r["steps"], 1, "the box was captured: {r}");

    let r = run(&mut s, "macro.play", json!({ "source": "// Demo\nrun(\"solid.cylinder\", #{ radius: 3, height: 5 })" }));
    matches(&played, &r, false);
    matches(&cleared, &run(&mut s, "macro.clear", json!({})), false);
    let r = run(&mut s, "macro.list", json!({}));
    matches(&listed, &r, false);

    // Save, open and delete round-trip through a real file (the same way cmd/macros_tests does).
    let dir = std::env::temp_dir().join(format!("sc-macro-schema-{}", std::process::id()));
    let path = dir.join("demo.rhai");
    run(&mut s, "macro.record", json!({ "name": "Demo" }));
    run(&mut s, "solid.sphere", json!({ "radius": 4 }));
    run(&mut s, "macro.stop", json!({}));
    let r = run(&mut s, "macro.save", json!({ "path": path.display().to_string() }));
    matches(&saved, &r, false);
    let r = run(&mut s, "macro.open", json!({ "path": path.display().to_string() }));
    matches(&opened, &r, false);
    assert_eq!(r["steps"], 1, "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}
