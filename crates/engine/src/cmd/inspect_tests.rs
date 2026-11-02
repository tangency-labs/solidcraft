use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn param<'a>(inspect: &'a Value, name: &str) -> &'a Value {
    inspect["params"].as_array().and_then(|a| a.iter().find(|p| p["name"] == name)).unwrap_or(&Value::Null)
}

fn listed<'a>(list: &'a Value, name: &str) -> &'a Value {
    list["parameters"].as_array().and_then(|a| a.iter().find(|p| p["name"] == name)).unwrap_or(&Value::Null)
}

/// #10747: `document.inspect` reports a parameter's `value` in millimetres (degrees for an
/// angle) like every other reply, with the parameter's own unit in `display_value`.
#[test]
fn inspect_reports_the_same_unit_as_every_other_reply() {
    let mut s = Session::default();
    run(&mut s, "document.units", json!({"units": "in"}));
    run(&mut s, "parameters.add", json!({"name": "L", "expression": "2"}));
    run(&mut s, "parameters.change", json!({"name": "L", "expression": "3"}));
    run(&mut s, "parameters.add", json!({"name": "ang", "expression": "30 deg"}));
    let i = run(&mut s, "document.inspect", json!({}));
    // A length is millimetres, whatever the parameter's own unit, and free of float noise.
    assert_eq!(param(&i, "L")["value"], 76.2, "{i}");
    // The parameter's own unit is in `display_value`, and `unit` still names it.
    assert_eq!(param(&i, "L")["display_value"], 3.0, "{i}");
    assert_eq!(param(&i, "L")["unit"], "in", "{i}");
    // An angle is degrees, like every other reply, and without the float noise.
    assert_eq!(param(&i, "ang")["value"], 30.0, "{i}");
    assert_eq!(param(&i, "ang")["display_value"], 30.0, "{i}");
    // The same numbers as `parameters.list` reports: one meaning of `value` in every reply.
    let l = run(&mut s, "parameters.list", json!({}));
    for name in ["L", "ang"] {
        assert_eq!(param(&i, name)["value"], listed(&l, name)["value"], "{name}: {i} {l}");
        assert_eq!(param(&i, name)["display_value"], listed(&l, name)["display_value"], "{name}: {i} {l}");
    }
}
