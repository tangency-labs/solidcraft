use serde_json::{Value, json};

use super::{LooseEnds, LooseEndsParams, TieLooseEndsParams, TiedLooseEnds};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A sketch being edited, as these commands need: a square of four separate lines, closed by
/// position only, so its four corners are untied.
fn loose_square(s: &mut Session) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "Square"}));
    let c = [[0, 0], [10, 0], [10, 10], [0, 10]];
    for i in 0..4 {
        run(s, "sketch.line", json!({"points": [c[i], c[(i + 1) % 4]], "infer": false, "connect": false}));
    }
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<LooseEndsParams>(), &["sketch"]);
    assert_keys(input_schema_for::<TieLooseEndsParams>(), &["points", "sketch"]);
}

/// A call that fails today gets the same message in strict. These commands run only while a
/// sketch is being edited, so each call runs in one (`assert_strict_keeps_todays_errors` runs
/// after a finished sketch).
#[test]
fn strict_keeps_todays_errors() {
    let many = vec!["p1"; 10_001];
    for (id, p) in [
        ("sketch.loose_ends", json!({"sketch": true})),
        ("sketch.loose_ends", json!({"sketch": -1})),
        ("sketch.loose_ends", json!({"sketch": 1.5})),
        ("sketch.loose_ends", json!({"sketch": "Nope"})),
        ("sketch.loose_ends.tie", json!({"sketch": {}})),
        ("sketch.loose_ends.tie", json!({"points": "p1"})),
        ("sketch.loose_ends.tie", json!({"points": [1]})),
        ("sketch.loose_ends.tie", json!({"points": ["nope"]})),
        ("sketch.loose_ends.tie", json!({"points": many})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            loose_square(&mut s);
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// #10959: the report and the tie reply as their output schemas say, in Warn without warnings
/// for good calls and with them for a misspelt key, with open ends, untied ends touching a
/// point and a curve, and ties of some ends and then the rest.
#[test]
fn loose_end_commands_reply_with_their_schemas() {
    let (report, tied) = (output_schema_for::<LooseEnds>(), output_schema_for::<TiedLooseEnds>());
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    loose_square(&mut s);
    // A line from the middle of the top side: one end touches a curve, the other nothing.
    run(&mut s, "sketch.line", json!({"points": [[5, 10], [5, 20]], "infer": false, "connect": false}));
    let r = run(&mut s, "sketch.loose_ends", json!({}));
    matches(&report, &r, false);
    let untied = r["untied"].as_array().cloned().unwrap_or_default();
    assert_eq!(r["open"].as_array().map(Vec::len), Some(1), "{r}");
    assert!(untied.iter().any(|u| u["touches"]["point"].is_string()) && untied.iter().any(|u| u["touches"]["curve"].is_string()), "{r}");
    for u in &untied {
        let touches = u["touches"].as_object().map(|t| t.keys().cloned().collect::<Vec<_>>());
        assert!(matches!(touches.as_deref(), Some([k]) if k == "point" || k == "curve"), "{u}");
    }
    matches(&report, &run(&mut s, "sketch.loose_ends", json!({"sketch": "Square"})), false);
    matches(&report, &run(&mut s, "sketch.loose_ends", json!({"skech": "Square"})), true);

    let first = untied.first().map(|u| u["point"].clone()).unwrap_or_default();
    let r = run(&mut s, "sketch.loose_ends.tie", json!({"points": [first]}));
    matches(&tied, &r, false);
    assert_eq!(r["constraints"].as_array().map(Vec::len), Some(1), "{r}");
    let r = run(&mut s, "sketch.loose_ends.tie", json!({"point": []}));
    matches(&tied, &r, true);
    assert_eq!(r["untied"], json!([]), "every end is tied: {r}");
    matches(&tied, &run(&mut s, "sketch.loose_ends.tie", json!({"sketch": "Square", "points": null})), false);
}
