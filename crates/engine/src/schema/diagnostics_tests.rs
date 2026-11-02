use serde_json::{Value, json};

use super::{
    CrashReport, CrashReportList, CrashReportParams, CrashReportsClearParams, CrashReportsCleared, CrashReportsParams, CrashTestMode, CrashTestParams,
};
use crate::Session;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<CrashReportsParams>(), &["limit"]);
    assert_keys(input_schema_for::<CrashReportParams>(), &["id"]);
    assert_keys(input_schema_for::<CrashReportsClearParams>(), &["id"]);
    assert_keys(input_schema_for::<CrashTestParams>(), &["mode"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[("diagnostics.crash_report", json!({"id": "no-such-report"}))]);
}

/// The crash test runs only with SOLIDCRAFT_CRASH_TEST set, so its mode is checked here: the
/// schema lists the three modes (one `const` each, with what it breaks), the type reads them,
/// and anything else gets the handler's message.
#[test]
fn the_crash_test_mode_is_one_of_three() {
    let schema = input_schema_for::<CrashTestParams>();
    let schema = schema.as_value();
    let listed: Vec<&str> = schema["$defs"]["CrashTestMode"]["oneOf"].as_array().into_iter().flatten().filter_map(|m| m["const"].as_str()).collect();
    assert_eq!(listed, ["panic", "kernel", "abort"], "{schema}");
    for (mode, want) in [("panic", CrashTestMode::Panic), ("kernel", CrashTestMode::Kernel), ("abort", CrashTestMode::Abort)] {
        let p = json!({ "mode": mode });
        assert_eq!(serde_json::from_value::<CrashTestParams>(p.clone()).ok().map(|p| p.mode), Some(want), "{mode}");
        assert!(conforms(schema, schema, &p), "{p}");
    }
    for mode in [json!("Panic"), json!("explode"), json!(""), json!(1), json!(null)] {
        let p = json!({ "mode": mode });
        let error = serde_json::from_value::<CrashTestParams>(p.clone()).err().map(|e| e.to_string()).unwrap_or_default();
        assert!(error.contains("must be panic, kernel or abort"), "{p}: {error}");
        assert!(!mode.as_str().is_some_and(|m| listed.contains(&m)), "{p}");
    }
}

/// A report in this test process's crash folder, as the app writes one, with a design and a
/// replay beside it.
fn write_report(id: &str) {
    let dir = crate::crash::default_dir().unwrap_or_else(|| panic!("no crash folder")).join(id);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
    let report = json!({
        "format": 1, "id": id, "kind": "command_panic", "signature": "00000000deadbeef", "count": 2,
        "first_seen": "2026-11-01T12:00:00Z", "last_seen": "2026-11-01T12:05:00Z",
        "app": {"version": "0.29.0", "commit": "abc", "built": "2026-10-31"},
        "system": {"os": "windows", "arch": "x86_64", "environment": ""}, "session": {"uptime_s": 60},
        "message": "an internal error", "location": "here.rs:1", "thread": "main", "backtrace": ["f (here.rs:1)"],
        "command": {"command": "solid.box", "params": {}}, "base": "new", "recent_commands": [],
        "files": {"design": "design.sc", "replay": "replay.json"}, "reproduce": format!("solidcraft-cli crashes replay {id}"), "notes": [],
        "startup_phase": "running"
    });
    for (name, text) in [("report.json", report.to_string()), ("design.sc", "{}".into()), ("replay.json", "[]".into())] {
        std::fs::write(dir.join(name), text).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// #10909: listing, reading and deleting reports reply as their output schemas say, in Warn
/// without warnings for good calls and with them for a misspelt key; strict refuses an `id`
/// that isn't text, which the handler would read as no id and so delete every report.
#[test]
fn crash_report_commands_reply_with_their_schemas() {
    let id = "20261101-120000-00000000deadbeef";
    write_report(id);
    let (list_schema, report_schema, cleared_schema) =
        (output_schema_for::<CrashReportList>(), output_schema_for::<CrashReport>(), output_schema_for::<CrashReportsCleared>());
    let mut s = Session { param_check: ParamCheck::Warn, ..Session::default() };
    let run = |s: &mut Session, id: &str, p: Value| s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    let matches = |schema: &schemars::Schema, r: &Value, warned: bool| {
        let schema = schema.as_value();
        assert!(conforms(schema, schema, r), "{r} doesn't match {schema}");
        assert_eq!(r.get("param_warnings").is_some(), warned, "{r}");
    };
    for p in [json!({}), json!({"limit": 5}), json!({"limit": 0}), json!({"limit": 5000})] {
        let r = run(&mut s, "diagnostics.crash_reports", p.clone());
        matches(&list_schema, &r, false);
        let listed = r["reports"].as_array().is_some_and(|a| a.iter().any(|x| x["id"] == id));
        assert_eq!(listed, p["limit"] != json!(0), "{p}: {r}");
    }
    matches(&list_schema, &run(&mut s, "diagnostics.crash_reports", json!({"limit": "5"})), true);
    let r = run(&mut s, "diagnostics.crash_report", json!({"id": id}));
    matches(&report_schema, &r, false);
    assert_eq!((r["id"].as_str(), r["count"].as_u64(), r["startup_phase"].as_str()), (Some(id), Some(2), Some("running")), "{r}");
    assert!(r["design_path"].is_string() && r["replay_path"].is_string() && r["path"].is_string(), "{r}");
    matches(&report_schema, &run(&mut s, "diagnostics.crash_report", json!({"id": id, "ids": [id]})), true);
    matches(&cleared_schema, &run(&mut s, "diagnostics.crash_reports.clear", json!({"id": "20261101-120000-0000000000000000"})), false);
    let mut strict = Session { param_check: ParamCheck::Strict, ..Session::default() };
    let error = strict.execute("diagnostics.crash_reports.clear", &json!({"id": 5})).err().map(|e| e.to_string()).unwrap_or_default();
    assert!(error.contains("`id`"), "{error}");
    let r = run(&mut s, "diagnostics.crash_reports.clear", json!({"id": id}));
    matches(&cleared_schema, &r, false);
    assert_eq!(r["deleted"], 1, "the report was still there to delete: {r}");
}
