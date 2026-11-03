use serde_json::{Value, json};

use super::PostParams;
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

/// As in the command's own tests: one tool, a setup, a contour and a pocket
/// over a plate.
fn cam(s: &mut Session) {
    run(
        s,
        "cam.tool.define",
        json!({"number": 1, "diameter": 6, "flute_length": 20, "spindle_rpm": 8000, "feed": 600, "plunge_feed": 200, "stepdown": 3}),
    );
    run(s, "cam.setup", json!({"name": "Top", "work_offset": 1}));
    run(
        s,
        "cam.op.contour",
        json!({"setup": "Top", "name": "Outside", "tool": 1, "outer": [[0,0],[40,0],[40,30],[0,30]], "side": "outside", "bottom": -6, "stepdown": 3}),
    );
    run(
        s,
        "cam.op.pocket",
        json!({"setup": "Top", "name": "Pocket", "tool": 1, "outer": [[5,5],[35,5],[35,25],[5,25]], "stepover": 2.5, "bottom": -3, "stepdown": 3}),
    );
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PostParams>(), &["post", "program", "setup"]);
}

#[test]
fn strict_keeps_todays_errors() {
    // Type mismatches are absent on purpose: strict reports them better
    // ("`setup` must be text") than the handler, which ignored the junk.
    assert_strict_keeps_todays_errors(&[("cam.post", json!({"setup": "Nope"})), ("cam.post", json!({}))]);
}

#[test]
fn a_post_replies_with_its_schema() {
    let schema = output_schema_for::<crate::schema::cam::PostResult>();
    let matches = |r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    cam(&mut s);
    let r = run(&mut s, "cam.post", json!({"setup": "Top"}));
    matches(&r, false);
    assert_eq!(r["format"], "gcode");
    assert_eq!(r["operations"], 2);
    assert_eq!(r["readback"]["within"], true);
    let renamed = run(&mut s, "cam.post", json!({"setup": "Top", "post": "rs274", "program": "Plate rev 2"}));
    matches(&renamed, false);
    assert_eq!(renamed["post"], "rs274");
    // A misspelt key warns in Warn mode and still posts.
    let warned = run(&mut s, "cam.post", json!({"setup": "Top", "postname": "x"}));
    matches(&warned, true);
}
