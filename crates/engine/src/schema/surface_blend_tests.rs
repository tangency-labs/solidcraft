use serde_json::{Value, json};

use super::BlendParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// Boxes `A` and `B`, with a free edge of each facing the other.
fn boxes(s: &mut Session) {
    run(s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "A"}));
    run(s, "solid.box", json!({"length": 40, "width": 10, "height": 10, "corner": [0, 20, 5], "body_name": "B"}));
}

fn from() -> Value {
    json!({"body": "A", "edges": [[20, 10, 10]]})
}

fn to() -> Value {
    json!({"body": "B", "edges": [[20, 20, 5]]})
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<BlendParams>(),
        &["align", "body_name", "body_names", "bulge", "continuity", "from", "name", "participants", "seam", "stitch", "to"],
    );
}

/// A call that fails today gets the same message in strict.
#[test]
fn strict_keeps_todays_errors() {
    let with = |key: &str, value: Value| {
        let mut p = json!({"from": from(), "to": to()});
        p[key] = value;
        p
    };
    let point = json!([0, 0, 0]);
    for p in [
        with("from", json!(5)),
        with("from", json!(null)),
        with("from", json!({"edges": [point]})),
        with("from", json!({"body": 5, "edges": [point]})),
        with("to", json!({"body": "B"})),
        with("to", json!({"body": "B", "edges": []})),
        with("to", json!({"body": "B", "edges": [["x", 0, 0]]})),
        with("continuity", json!(null)),
        with("continuity", json!("g1")),
        with("continuity", json!(["g1"])),
        with("continuity", json!(["g1", "g7"])),
        with("continuity", json!(["g1", 5])),
        with("bulge", json!(null)),
        with("bulge", json!(1)),
        with("bulge", json!([0])),
        with("bulge", json!([0, 5])),
        with("bulge", json!(["x", 0])),
        with("align", json!(5)),
        with("align", json!([point])),
        with("align", json!([point, "x"])),
        with("seam", json!(5)),
        with("seam", json!([point, point, point])),
        with("seam", json!([point, [0, 0]])),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            s.execute("surface.blend", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// A malformed point the handler would drop, and a misspelt key inside an end, are refused.
#[test]
fn strict_refuses_what_the_handler_silently_drops() {
    let point = json!([20, 10, 10]);
    for (side, message) in [
        (json!({"body": "A", "edges": [point, "x"]}), "`from.edges`: a point must be [x, y, z]"),
        (json!({"body": "A", "edges": [point], "edge": []}), "`from` has an unknown key `edge`"),
    ] {
        let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
        boxes(&mut s);
        let error = s.execute("surface.blend", &json!({"from": side, "to": to()})).unwrap_err().to_string();
        assert!(error.contains(message), "{error}");
    }
}

/// A blend with every key replies with `FeatureResult`, in Warn without warnings for a good call
/// and with them for a misspelt key.
#[test]
fn a_blend_replies_with_the_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    boxes(&mut s);
    let r = run(
        &mut s,
        "surface.blend",
        json!({"from": from(), "to": to(), "continuity": ["g0", "G1"], "bulge": [0.5, 0], "align": null, "seam": null, "stitch": false, "name": "Blend", "body_name": "Sheet"}),
    );
    matches(&r, false);
    assert_eq!(r["created_bodies"], json!(["Sheet"]), "{r}");
    matches(&run(&mut s, "surface.blend", json!({"from": from(), "to": to(), "bulg": [0, 0]})), true);
}
