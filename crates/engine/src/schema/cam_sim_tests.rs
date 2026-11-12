//! Tests of the `cam.simulate` contract: its keys, and replies that match its output schema.

use serde_json::{Value, json};

use super::{CamSimulateParams, CamSimulateReply};
use crate::Session;
use crate::schema::cam::CamJobReply;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CamSimulateParams>(), &["job", "job_path", "path", "resolution", "stock_stl", "then"]);
}

/// Strict refuses a misspelt key; replies match the output schemas, in Warn without warnings.
#[test]
fn replies_match_the_output_schema() {
    let matches = |schema: schemars::Schema, r: &Value| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
    };
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 10, "corner": [-10, -10, -10]}));
    let job = json!({
        "machine": "3axis",
        "post": "linuxcnc_4axis",
        "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 4, "flute_length": 10, "spindle_rpm": 10000, "feed": 600, "plunge_feed": 150}],
        "operations": [{
            "name": "face", "type": "pocket", "tool": 1, "polygon": [[-10, -10], [10, -10], [10, 10], [-10, 10]],
            "heights": {"clearance": 20, "retract": 5, "top": 1, "bottom": 0}
        }]
    });
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-cam-sim-schema-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.ngc").to_string_lossy().into_owned();
    let r = run(&mut s, "cam.job", json!({"job": job, "output": file, "simulate": true}));
    matches(output_schema_for::<CamJobReply>(), &r);
    assert!(r["simulation"].is_object(), "{r}");
    let stl = dir.join("stock.stl").to_string_lossy().into_owned();
    let r = run(&mut s, "cam.simulate", json!({"path": file, "job": job, "resolution": 0.2, "stock_stl": stl}));
    matches(output_schema_for::<CamSimulateReply>(), &r);
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let e = strict.execute("cam.simulate", &json!({"path": file, "job": job, "resolutoin": 0.2})).unwrap_err();
    assert!(e.to_string().contains("resolutoin"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
