use serde_json::{Value, json};

use super::{
    AppearanceAssignParams, AppearanceAssignResult, AppearanceLibraryParams, AppearanceLibraryResult, AppearanceListParams, AppearanceListResult,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<AppearanceAssignParams>(),
        &["appearance", "bodies", "body", "color", "components", "faces", "name", "opacity", "pbr"],
    );
    assert_keys(input_schema_for::<AppearanceLibraryParams>(), &[]);
    assert_keys(input_schema_for::<AppearanceListParams>(), &["body"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("appearance.assign", json!({"faces": [[1, 2]]})),
        ("appearance.assign", json!({"faces": [{"body": "Box1"}]})),
        ("appearance.assign", json!({"color": "red"})),
        ("appearance.assign", json!({"color": [0, 128, 300]})),
        ("appearance.assign", json!({"color": "#ff0000", "name": ""})),
        ("appearance.assign", json!({"color": "#ff0000", "pbr": {"metallic": 1}})),
        ("appearance.assign", json!({"color": "#ff0000", "pbr": {"metallic": "1", "roughness": 0.5, "dielectric_ior": 1.5}})),
        ("appearance.assign", json!({"color": "#ff0000", "pbr": {"metallic": 2, "roughness": 0.5, "dielectric_ior": 1.5}})),
    ]);
}

/// A library look, a starter recipe, a colour with PBR on a face, a cleared body, the library
/// and the list with and without `body`, in Warn: each reply matches its output schema and
/// carries no warning; a misspelt key's warning still matches.
#[test]
fn replies_match_the_output_schema() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let mut run = |id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: schemars::Schema, id: &str, reply: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, reply), "{id}: {reply} doesn't match {schema}");
    };
    let assigned = |reply: &Value| {
        assert!(reply.get("param_warnings").is_none(), "{reply}");
        matches(output_schema_for::<AppearanceAssignResult>(), "appearance.assign", reply);
    };
    let made = run("solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0]}));
    let body = made["created_bodies"][0].as_str().unwrap_or_default().to_string();
    let library = run("appearance.library", json!({}));
    assert!(library.get("param_warnings").is_none(), "{library}");
    matches(output_schema_for::<AppearanceLibraryResult>(), "appearance.library", &library);
    let entries = library["appearances"].as_array().cloned().unwrap_or_default();
    let named = |pick: &dyn Fn(&Value) -> bool| entries.iter().find(|e| pick(e)).and_then(|e| e["name"].as_str()).unwrap_or_default().to_string();
    let plain = named(&|e| e.get("id").is_none() && e.get("pbr").is_none());
    let finish = named(&|e| e.get("id").is_none() && e.get("pbr").is_some());
    let starter = entries.iter().find(|e| e["available"] == true).and_then(|e| e["id"].as_str()).unwrap_or_default().to_string();
    assert!(!plain.is_empty() && !finish.is_empty() && !starter.is_empty(), "{library}");
    assigned(&run("appearance.assign", json!({"bodies": [body], "appearance": plain, "opacity": 0.5})));
    assigned(&run("appearance.assign", json!({"bodies": body, "appearance": finish})));
    assigned(&run("appearance.assign", json!({"bodies": [body], "appearance": starter})));
    let face = run(
        "appearance.assign",
        json!({"faces": [{"point": [5, 5, 10], "body": body}], "color": [200, 40, 40], "name": "Signal red",
               "pbr": {"metallic": 0, "roughness": 0.4, "dielectric_ior": 1.5}}),
    );
    assert_eq!(face["faces"][0]["point"], json!([5.0, 5.0, 10.0]), "{face}");
    assigned(&face);
    let all = run("appearance.list", json!({}));
    assert!(all.get("param_warnings").is_none() && all.get("body").is_none(), "{all}");
    matches(output_schema_for::<AppearanceListResult>(), "appearance.list", &all);
    let one = run("appearance.list", json!({"body": body}));
    assert_eq!(one["body"]["faces"].as_array().map(Vec::len), Some(1), "{one}");
    matches(output_schema_for::<AppearanceListResult>(), "appearance.list", &one);
    let cleared = run("appearance.assign", json!({"bodies": [body]}));
    assert!(cleared["appearance"].is_null(), "{cleared}");
    assigned(&cleared);
    let warned = run("appearance.list", json!({"bodyy": body}));
    assert!(warned["param_warnings"].as_array().is_some_and(|w| !w.is_empty()), "{warned}");
    matches(output_schema_for::<AppearanceListResult>(), "appearance.list", &warned);
}

/// A colour's text pattern allows what `inspect::color_param` reads: spaces at either end, as
/// `trim` drops them, any number of `#` before the three bytes, and a byte written as `+` and
/// one digit, as `from_str_radix` reads it. `conforms` doesn't read patterns, so the pattern's
/// text is pinned here, next to the reader's answers.
#[test]
fn the_colour_pattern_allows_what_the_reader_trims() {
    for text in ["#ff00ff", "ff00ff", "  #FF00ff\t", "##ff00ff"] {
        assert_eq!(crate::cmd::inspect::color_param(&json!(text)), Some([255, 0, 255]), "{text}");
    }
    assert_eq!(crate::cmd::inspect::color_param(&json!("# ff00ff")), None, "a space after the `#` isn't trimmed");
    assert_eq!(crate::cmd::inspect::color_param(&json!("+fff00")), Some([15, 255, 0]), "`from_str_radix` takes a leading `+`");
    let schema = schemars::schema_for!(super::ColorParam);
    let ws = crate::schema::types::whitespace_class();
    let pattern = format!("^[{ws}]*#*(?:[0-9A-Fa-f]{{2}}|\\+[0-9A-Fa-f]){{3}}[{ws}]*$");
    assert_eq!(schema.as_value()["anyOf"][0]["pattern"], json!(pattern), "{}", schema.as_value());
}
