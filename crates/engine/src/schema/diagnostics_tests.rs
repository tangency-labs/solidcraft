use serde_json::json;

use super::ProfileParams;
use crate::Session;
use crate::schema::input_schema_for;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ProfileParams>(), &["enabled", "reset", "top", "trace"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("diagnostics.profile", json!({"enabled": "yes"})),
        ("diagnostics.profile", json!({"reset": 1})),
        ("diagnostics.profile", json!({"top": -1})),
        ("diagnostics.profile", json!({"top": 2.5})),
        ("diagnostics.profile", json!({"trace": 5})),
    ]);
}

/// The profiler records commands, the features they compute and the kernel below them, and
/// reports, resets and writes a Chrome trace through the command.
#[test]
fn diagnostics_profile_reports_commands_features_and_the_kernel() {
    let _serial = super::PROFILE_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut s = Session::default();
    s.execute("diagnostics.profile", &json!({"reset": true, "enabled": true})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    let r = s.execute("diagnostics.profile", &json!({"top": 0})).unwrap();
    assert_eq!((r["compiled"].as_bool(), r["enabled"].as_bool()), (Some(true), Some(true)), "{r}");
    let rows = r["rows"].as_array().unwrap();
    let row = |cat: &str, name: &str| rows.iter().find(|x| x["category"] == cat && x["name"] == name);
    let cmd = row("command", "solid.box").unwrap_or_else(|| panic!("{r}"));
    assert!(row("feature", "BoxFeature").is_some() && row("doc", "evaluate").is_some(), "{r}");
    assert!(row("kernel", "box_solid").is_some(), "{r}");
    assert!(cmd["self_ms"].as_f64().unwrap() <= cmd["total_ms"].as_f64().unwrap(), "{r}");
    assert_eq!(r["rows_total"].as_u64(), Some(rows.len() as u64));
    // `top` keeps the rows with the most self time.
    let one = s.execute("diagnostics.profile", &json!({"top": 1})).unwrap();
    assert_eq!(one["rows"].as_array().map(Vec::len), Some(1));
    // A trace file, then stop and forget.
    let dir = std::env::temp_dir().join(format!("solidcraft-profile-trace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("trace.json").to_string_lossy().to_string();
    let since_trace = std::time::Instant::now();
    let t = s.execute("diagnostics.profile", &json!({"trace": path, "enabled": false, "reset": true})).unwrap();
    assert_eq!((t["trace"].as_str(), t["enabled"].as_bool()), (Some(path.as_str()), Some(false)));
    let trace: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(trace["traceEvents"].as_array().unwrap().iter().any(|e| e["ph"] == "X" && e["name"] == "solid.box"), "{trace}");
    // Other tests run commands (this one among them) while it records, so the rows after the
    // reset can hold theirs too. The wall time shows the reset: it starts again at the call that
    // reset it, whose own span ends after the reset and is the one row certain to be there.
    let after = s.execute("diagnostics.profile", &json!({})).unwrap();
    assert!(after["wall_ms"].as_f64().unwrap() <= since_trace.elapsed().as_secs_f64() * 1000.0 + 0.001, "{after}");
    let rows = after["rows"].as_array().unwrap();
    assert!(rows.iter().any(|x| x["category"] == "command" && x["name"] == "diagnostics.profile"), "{after}");
    assert_eq!(after["enabled"], json!(false), "{after}");
    let _ = std::fs::remove_dir_all(&dir);
}
