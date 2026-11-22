use serde_json::{Value, json};

use super::{Copied, CopyParams, CutParams, PasteParams, Pasted};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A sketch `A` being edited, as these commands need, with a line and a circle; the line is
/// copied. Their ids.
fn drawing(s: &mut Session) -> (String, String) {
    run(s, "sketch.create", json!({"plane": "XY", "name": "A"}));
    let line = run(s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))["curves"][0].as_str().map(str::to_string).unwrap_or_default();
    let circle =
        run(s, "sketch.circle.center", json!({"center": [30, 0], "radius": 4}))["curves"][0].as_str().map(str::to_string).unwrap_or_default();
    run(s, "sketch.copy", json!({"entities": [line]}));
    (line, circle)
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CopyParams>(), &["entities", "sketch"]);
    assert_keys(input_schema_for::<CutParams>(), &["entities", "sketch"]);
    assert_keys(input_schema_for::<PasteParams>(), &["at", "sketch", "translate"]);
}

/// A call that fails today gets the same message in strict. These commands run only while a
/// sketch is being edited, so each call runs in one (`assert_strict_keeps_todays_errors` runs
/// after a finished sketch).
#[test]
fn strict_keeps_todays_errors() {
    for (id, p) in [
        ("sketch.copy", json!({})),
        ("sketch.copy", json!({"entities": null})),
        ("sketch.copy", json!({"entities": ["nope"]})),
        ("sketch.copy", json!({"entities": "l1", "sketch": "Nope"})),
        ("sketch.cut", json!({"entities": ["nope"]})),
        ("sketch.paste", json!({"at": "x"})),
        ("sketch.paste", json!({"at": null})),
        ("sketch.paste", json!({"at": [1e9, 0]})),
        ("sketch.paste", json!({"at": "x", "translate": [1, 2]})),
        ("sketch.paste", json!({"translate": [1, 2, 3]})),
        ("sketch.paste", json!({"translate": {"x": 1}})),
        ("sketch.paste", json!({"sketch": "Nope"})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session::default();
            drawing(&mut s);
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// #10963: copy, cut and paste reply as their output schemas say, in Warn without warnings for
/// good calls and with them for a misspelt key: a copy of one curve and of the selection, pastes
/// without a position, at a point and moved, and a cut pasted back.
#[test]
fn clipboard_commands_reply_with_their_schemas() {
    let (copied, pasted) = (output_schema_for::<Copied>(), output_schema_for::<Pasted>());
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let (line, circle) = drawing(&mut s);
    let r = run(&mut s, "sketch.copy", json!({"entities": [line, circle], "sketch": "A"}));
    matches(&copied, &r, false);
    assert_eq!((r["curves"].as_u64(), r["center"].as_array().map(Vec::len)), (Some(2), Some(2)), "{r}");
    let r = run(&mut s, "sketch.paste", json!({}));
    matches(&pasted, &r, false);
    assert_eq!(r["curves"].as_array().map(Vec::len), Some(2), "{r}");
    // The pasted curves are selected: a copy with no `entities` takes them.
    matches(&copied, &run(&mut s, "sketch.copy", json!({})), false);
    matches(&pasted, &run(&mut s, "sketch.paste", json!({"at": [50, 50]})), false);
    let r = run(&mut s, "sketch.paste", json!({"translate": {"x": 5, "y": -5}, "sketch": "A"}));
    matches(&pasted, &r, false);
    assert_eq!(r["translate"], json!([5.0, -5.0]), "{r}");
    matches(&copied, &run(&mut s, "sketch.cut", json!({"entities": circle})), false);
    matches(&pasted, &run(&mut s, "sketch.paste", json!({})), false);

    matches(&copied, &run(&mut s, "sketch.copy", json!({"entities": [line], "entites": [line]})), true);
    matches(&copied, &run(&mut s, "sketch.cut", json!({"entities": [line], "sketc": "A"})), true);
    matches(&pasted, &run(&mut s, "sketch.paste", json!({"at": [0, 0], "tranlate": [1, 1]})), true);
}
