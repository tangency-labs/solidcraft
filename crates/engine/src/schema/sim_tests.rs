use serde_json::json;

use super::{ClearParams, ClearReply, RunParams, RunReply, SetupParams, SetupReply, StatusParams, StatusReply};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<SetupParams>(), &["gcode", "initial_tool", "memory_mb", "rapid_mm_min", "resolution", "stock", "tools"]);
    assert_keys(input_schema_for::<RunParams>(), &["budget_s", "time", "to"]);
    assert_keys(input_schema_for::<StatusParams>(), &["limit", "offset"]);
    assert_keys(input_schema_for::<ClearParams>(), &[]);
}

#[test]
fn replies_match_the_output_schemas() {
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let setup = json!({
        "stock": {"box": {"min": [0, 0, -10], "max": [60, 20, 0]}},
        "tools": [{"id": 0, "diameter": 5, "flute_length": 20}],
        "gcode": "G0 X10 Y10.03 Z5\nG1 Z-4 F100\nG1 X50\nG0 Z5\nG0 X10 Y10.03",
        "resolution": 0.2
    });
    let reply = s.execute("sim.setup", &setup).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<SetupReply>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply} doesn't match the setup reply");
    let reply = s.execute("sim.run", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<RunReply>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply} doesn't match the run reply");
    let reply = s.execute("sim.status", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<StatusReply>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply} doesn't match the status reply");
    let reply = s.execute("sim.clear", &json!({})).unwrap_or_else(|e| panic!("{e}"));
    let schema = output_schema_for::<ClearReply>();
    assert!(conforms(schema.as_value(), schema.as_value(), &reply), "{reply} doesn't match the clear reply");
}
