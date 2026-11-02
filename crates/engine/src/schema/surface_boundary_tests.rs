use serde_json::{Value, json};

use super::BoundaryParams;
use crate::Session;
use crate::schema::features::FeatureResult;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A box `Box`, then a sketch `Flat` being edited with a 10 mm square of four separate lines:
/// their ids, bottom, top, left, right.
fn square(s: &mut Session) -> [String; 4] {
    run(s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [40, 40, 0], "body_name": "Box"}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Flat"}));
    let line = |s: &mut Session, a: [i32; 2], b: [i32; 2]| {
        run(s, "sketch.line", json!({"points": [a, b], "infer": false, "connect": false}))["created"]["curves"][0]
            .as_str()
            .map(str::to_string)
            .unwrap_or_default()
    };
    [line(s, [0, 0], [10, 0]), line(s, [0, 10], [10, 10]), line(s, [0, 0], [0, 10]), line(s, [10, 0], [10, 10])]
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<BoundaryParams>(),
        &["body_name", "body_names", "continuity", "crown", "curves", "name", "operation", "participants", "samples", "sketch", "u", "v"],
    );
}

/// A call that fails today gets the same message in strict. These need a sketch being edited
/// and its curves, so each runs on one (`assert_strict_keeps_todays_errors` runs after a finished
/// sketch, with no curves to name).
#[test]
fn strict_keeps_todays_errors() {
    let mut s = Session::default();
    let [a, b, ..] = square(&mut s);
    let many: Vec<Value> = (0..33).map(|_| json!(a)).collect();
    let point = json!([0, 0, 0]);
    for p in [
        json!({"curves": [a]}),
        json!({"curves": 5}),
        json!({"curves": [a, 5]}),
        json!({"curves": [a, b], "continuity": "g7"}),
        json!({"curves": [a, b], "continuity": ["g1", 5]}),
        json!({"curves": [a, b], "continuity": 5}),
        json!({"curves": [a, b], "crown": 5}),
        json!({"curves": [a, b], "samples": 7}),
        json!({"curves": [a, b], "samples": 8.5}),
        json!({"curves": [a, b], "sketch": null}),
        json!({"curves": [a, b], "sketch": true}),
        json!({"u": "l1"}),
        json!({"u": [42, a]}),
        json!({"u": [{}, a]}),
        json!({"u": [{"curves": [], "sketch": "Flat"}, a]}),
        json!({"u": [{"body": 5, "edges": [point]}, a]}),
        json!({"u": [{"body": "Box", "edges": []}, a]}),
        json!({"u": [{"body": "Box", "edges": [["x", 0, 0]]}, a]}),
        json!({"u": [{"point": [0, 0]}, a]}),
        json!({"u": [{"curves": [a], "body": "Box", "edges": [point]}, b]}),
        json!({"u": [{"curves": [a], "continuity": "g1"}, b]}),
        json!({"u": [{"body": "Box", "edges": [point], "continuity": "g3"}, a]}),
        json!({"u": [{"body": "Box", "edges": [point], "continuity": "g9"}, a]}),
        json!({"u": many}),
        json!({"u": [a, b], "v": "x"}),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session { param_check: level, ..Session::default() };
            square(&mut s);
            s.execute("surface.boundary", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #11083: both forms reply with `FeatureResult`, in Warn without warnings for good calls and
/// with them for a misspelt key: a patch between two sides of the square with every loop key,
/// and the square as a network.
#[test]
fn a_boundary_surface_replies_with_the_feature_result() {
    let schema = output_schema_for::<FeatureResult>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let [bottom, top, left, right] = square(&mut s);
    let r = run(
        &mut s,
        "surface.boundary",
        json!({"sketch": "Flat", "curves": [bottom, top], "continuity": ["g1", "G2"], "crown": 0.5, "samples": 16, "name": "Patch", "body_name": "Sheet"}),
    );
    matches(&r, false);
    assert_eq!(r["created_bodies"], json!(["Sheet"]), "{r}");
    let r = run(&mut s, "surface.boundary", json!({"u": [bottom, {"sketch": "Flat", "curves": [top]}], "v": [left, right], "operation": "new"}));
    matches(&r, false);
    matches(&run(&mut s, "surface.boundary", json!({"curves": [bottom, top], "crwn": 1})), true);
}
