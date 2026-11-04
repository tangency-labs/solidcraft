use serde_json::{Value, json};

use super::{ParameterSweep, ParameterSweepParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// A 40 x 30 x 5 plate (its own component) with a 6 mm hole at x = `hx` (a parameter, 20 mm),
/// and a pin (another component) held on the hole by a rigid joint, as in the command's tests.
fn pinned(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "hx", "expression": "20 mm"}));
    run(s, "component.create", json!({"name": "Plate"}));
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 5, "corner": [0, 0, 0]}));
    run(s, "solid.hole", json!({"position": ["hx", 15, 5], "diameter": 6}));
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": "Pin"}));
    run(s, "solid.cylinder", json!({"base": [100, 0, 0], "axis": [0, 0, 1], "radius": 3, "height": 12, "operation": "new"}));
    run(s, "component.activate", json!({"component": "root"}));
    run(
        s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "Plate", "circle": [23, 15, 4.5]}, "b": {"occurrence": "Pin", "circle": [103, 0, 1]}, "name": "Seat"}),
    );
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ParameterSweepParams>(), &["from", "parameter", "steps", "to", "values"]);
}

/// A call that fails today gets the same message in strict. The handler looks the parameter up
/// before it reads the values, so each call runs on a design that has one
/// (`assert_strict_keeps_todays_errors` runs on one that has none).
#[test]
fn strict_keeps_todays_errors() {
    let many: Vec<u32> = (0..201).collect();
    let long = "1".repeat(1001);
    for p in [
        json!({}),
        json!({"parameter": 5, "values": [1]}),
        json!({"parameter": "nope", "values": [1]}),
        json!({"parameter": "w"}),
        json!({"parameter": "w", "values": 5}),
        json!({"parameter": "w", "values": null}),
        json!({"parameter": "w", "values": []}),
        json!({"parameter": "w", "values": many}),
        json!({"parameter": "w", "values": [true]}),
        json!({"parameter": "w", "values": [" "]}),
        json!({"parameter": "w", "values": [long]}),
        json!({"parameter": "w", "values": [1], "from": 0}),
        json!({"parameter": "w", "from": 0}),
        json!({"parameter": "w", "from": "x", "to": 1}),
        json!({"parameter": "w", "from": null, "to": 1}),
        json!({"parameter": "w", "from": 0, "to": 2e9}),
        json!({"parameter": "w", "from": 0, "to": 1, "steps": 0}),
        json!({"parameter": "w", "from": 0, "to": 1, "steps": 1.5}),
        json!({"parameter": "w", "from": 0, "to": 1, "steps": "x"}),
        json!({"parameter": "w", "from": 0, "to": 1, "steps": 200}),
    ] {
        let error = |level: ParamCheck| {
            let mut s = Session::default();
            run(&mut s, "parameters.add", json!({"name": "w", "expression": "10 mm"}));
            s.param_check = level;
            s.execute("inspect.parameter_sweep", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #10977: a sweep replies as its output schema says, in Warn without warnings for good calls
/// and with them for a misspelt key: values the last of which loses the joint (each result with
/// its changes), an even sweep, and a value the parameter refuses (a result with an error).
#[test]
fn a_sweep_replies_with_its_schema() {
    let schema = output_schema_for::<ParameterSweep>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    pinned(&mut s);
    let r = run(&mut s, "inspect.parameter_sweep", json!({"parameter": "hx", "values": [10, "30 mm", 80]}));
    matches(&r, false);
    let far = &r["results"][2];
    assert_eq!(far["ok"], false, "{r}");
    assert!(far["joints_lost"].as_array().is_some_and(|j| !j.is_empty()), "{r}");
    assert_eq!(r["summary"]["values"], 3, "{r}");
    let r = run(&mut s, "inspect.parameter_sweep", json!({"parameter": "hx", "from": 10, "to": 30, "steps": 2}));
    matches(&r, false);
    assert_eq!(r["results"].as_array().map(Vec::len), Some(3), "{r}");
    let r = run(&mut s, "inspect.parameter_sweep", json!({"parameter": "hx", "values": ["1 +"], "steps": null}));
    matches(&r, false);
    assert!(r["results"][0]["error"].is_string() && r["results"][0].get("failed").is_none(), "{r}");
    matches(&run(&mut s, "inspect.parameter_sweep", json!({"parameter": "hx", "values": [20], "step": 2})), true);
}
