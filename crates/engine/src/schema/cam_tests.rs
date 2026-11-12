//! Tests of the CAM contracts: their keys, and replies that match their output schemas.

use serde_json::{Value, json};

use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

use super::{
    CamGcodeCheckParams, CamGcodeCheckReply, CamJobParams, CamJobReply, CamPostGetParams, CamPostGetReply, CamPostListParams, CamPostListReply,
};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<CamJobParams>(),
        &[
            "allow_unsafe",
            "dry_run",
            "height",
            "image",
            "images_dir",
            "job",
            "job_path",
            "output",
            "preview",
            "setup_sheet",
            "simulate",
            "simulate_resolution",
            "split_by_tool",
            "view",
            "width",
        ],
    );
    assert_keys(input_schema_for::<CamPostListParams>(), &[]);
    assert_keys(input_schema_for::<CamPostGetParams>(), &["name"]);
    assert_keys(input_schema_for::<CamGcodeCheckParams>(), &["path", "rapid_rate"]);
}

/// Strict refuses what the handler refuses, with the key named.
#[test]
fn strict_refuses_misspelt_keys_inside_the_job() {
    let mut s = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let job = json!({"machine": "3axis", "post": "linuxcnc", "tools": [], "operations": [{"name": "a", "type": "pocket", "tool": 1, "polygon": [[0, 0], [5, 0], [0, 5]], "heights": {"clearance": 1, "retract": 1, "top": 0, "bottom": 0}, "stepovr": 1}]});
    let e = s.execute("cam.job", &json!({"job": job, "dry_run": true})).unwrap_err();
    assert!(e.to_string().contains("stepovr"), "{e}");
}

/// Every command replies as its output schema says, in Warn without warnings.
#[test]
fn replies_match_the_output_schemas() {
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
        "work_offset": 2,
        "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 4, "flute_length": 10, "spindle_rpm": 10000, "feed": 600, "plunge_feed": 150}],
        "operations": [{
            "name": "outline", "type": "contour", "tool": 1, "polygon": [[-10, -10], [10, -10], [10, 10], [-10, 10]],
            "heights": {"clearance": 20, "retract": 5, "top": 0, "bottom": -3}, "stepdown": 1.5
        }]
    });
    let r = run(&mut s, "cam.job", json!({"job": job, "dry_run": true}));
    assert_eq!(r["passed"], true, "{r}");
    matches(output_schema_for::<CamJobReply>(), &r);
    matches(output_schema_for::<CamPostListReply>(), &run(&mut s, "cam.post.list", json!({})));
    matches(output_schema_for::<CamPostGetReply>(), &run(&mut s, "cam.post.get", json!({"name": "grblhal"})));
    let dir = solidcraft_io::vfs::temp_dir().join(format!("solidcraft-cam-schema-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("p.ngc");
    std::fs::write(&file, r["program_head"].as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n")).unwrap();
    let c = run(&mut s, "cam.gcode.check", json!({"path": file.to_string_lossy(), "rapid_rate": 5000}));
    matches(output_schema_for::<CamGcodeCheckReply>(), &c);
    let _ = std::fs::remove_dir_all(&dir);
}
