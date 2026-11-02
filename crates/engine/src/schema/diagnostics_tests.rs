use serde_json::{Value, json};

use super::{
    CrashReport, CrashReportList, CrashReportParams, CrashReportsClearParams, CrashReportsCleared, CrashReportsParams, CrashTestMode,
    CrashTestParams, KernelParams, ProfileParams, ValidationParams,
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
    assert_keys(input_schema_for::<ProfileParams>(), &["enabled", "reset", "top", "trace"]);
    assert_keys(input_schema_for::<ValidationParams>(), &["mode"]);
    assert_keys(input_schema_for::<KernelParams>(), &["choice"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("diagnostics.crash_report", json!({"id": "no-such-report"})),
        ("diagnostics.profile", json!({"enabled": "yes"})),
        ("diagnostics.profile", json!({"reset": 1})),
        ("diagnostics.profile", json!({"top": -1})),
        ("diagnostics.profile", json!({"top": 2.5})),
        ("diagnostics.profile", json!({"trace": 5})),
        ("diagnostics.validation", json!({"mode": "each step"})),
        ("diagnostics.validation", json!({"mode": true})),
        ("diagnostics.kernel", json!({"choice": "cpp"})),
        ("diagnostics.kernel", json!({"choice": 1})),
    ]);
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

/// Tests that set the process-wide kernel choice run one at a time, and put `solidcore` back
/// however they end (a failed assertion included).
struct KernelChoice(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

static KERNEL_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl KernelChoice {
    fn take() -> Self {
        KernelChoice(KERNEL_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

impl Drop for KernelChoice {
    fn drop(&mut self) {
        solidcraft_kernel::kernel_route::set_choice(solidcraft_kernel::kernel_route::Backend::Solidcore);
    }
}

/// diagnostics.kernel says the kernel in force and whether the environment chose it, and its
/// `choice` sets it (unless SOLIDCRAFT_KERNEL overrides it).
///
/// The choice is process-wide and the other tests in this binary build bodies meanwhile, so only
/// `solidcore` is set through the command; what `smlib` asks for is read from `kernel_asked`.
#[test]
fn diagnostics_kernel_reports_and_sets_the_choice() {
    use solidcraft_kernel::kernel_route as route;
    let _choice = KernelChoice::take();
    let mut s = Session::default();
    let r = s.execute("diagnostics.kernel", &json!({})).unwrap();
    let environment = route::overridden();
    assert_eq!(r, json!({ "choice": route::choice().id(), "environment": environment }));
    let r = s.execute("diagnostics.kernel", &json!({ "choice": "solidcore" })).unwrap();
    assert!(environment || r["choice"] == json!("solidcore"), "{r}");
    assert_eq!(crate::cmd::diagnostics::kernel_asked(&json!("solidcore"), false).unwrap(), route::Backend::Solidcore);
    assert_eq!(crate::cmd::diagnostics::kernel_asked(&json!("solidcore"), true).unwrap(), route::Backend::Solidcore);
    assert_eq!(crate::cmd::diagnostics::kernel_asked(&json!("smlib"), true).unwrap(), route::Backend::Smlib);
}

/// `smlib` on a build without the C++ kernel is refused as not available, whichever kernel is in
/// force, and leaves the choice alone: never an answer from the other kernel. Built with it, the
/// command would move the other tests' bodies, so `kernel_asked` alone is asked.
#[test]
fn diagnostics_kernel_refuses_smlib_on_a_build_without_it() {
    let _choice = KernelChoice::take();
    let refused = crate::cmd::diagnostics::kernel_asked(&json!("smlib"), false).unwrap_err();
    assert!(matches!(&refused, crate::EngineError::Disabled(cmd, _) if cmd == "diagnostics.kernel"), "{refused}");
    if !solidcraft_kernel::smlib_built() {
        let mut s = Session::default();
        let before = s.execute("diagnostics.kernel", &json!({})).unwrap();
        assert!(matches!(s.execute("diagnostics.kernel", &json!({"choice": "smlib"})), Err(crate::EngineError::Disabled(..))));
        assert_eq!(s.execute("diagnostics.kernel", &json!({})).unwrap(), before);
    }
}

/// An unknown or mistyped choice is an error that changes nothing; no hostile value panics.
#[test]
fn diagnostics_kernel_refuses_unknown_choices() {
    let _choice = KernelChoice::take();
    let mut s = Session::default();
    let before = s.execute("diagnostics.kernel", &json!({})).unwrap()["choice"].clone();
    let huge = "solidcore".repeat(100_000);
    for bad in [
        json!("truck"),
        json!("SMLIB"),
        json!(""),
        json!(huge),
        json!("routed"),
        json!("routed\0"),
        json!(5),
        json!(true),
        json!(["smlib"]),
        json!({"choice": "smlib"}),
    ] {
        match s.execute("diagnostics.kernel", &json!({ "choice": bad })) {
            Err(crate::EngineError::BadParams { .. }) => {}
            other => panic!("`{}` was not refused: {:?}", bad.as_str().map_or(0, str::len), other.map_err(|e| e.to_string())),
        }
    }
    for v in crate::tests::hostile_values() {
        for p in [json!({ "choice": v.clone() }), v] {
            if let Err(crate::EngineError::Internal(_, msg)) = s.execute("diagnostics.kernel", &p) {
                panic!("diagnostics.kernel {p}: {msg}");
            }
        }
    }
    assert_eq!(s.execute("diagnostics.kernel", &json!({})).unwrap()["choice"], before);
}

/// Features are checked once each by default; the command reads and sets the mode, and the mode
/// is the user's: it stays when another design is started and when everything is computed again.
#[test]
fn validation_defaults_to_each_feature_and_round_trips_through_the_command() {
    use solidcraft_kernel::Validation;
    let mut s = Session::default();
    assert_eq!(s.validation(), Validation::EachFeature);
    assert_eq!(s.execute("diagnostics.validation", &json!({})).unwrap(), json!({"mode": "each_feature", "previous": "each_feature"}));
    let r = s.execute("diagnostics.validation", &json!({"mode": "each_step"})).unwrap();
    assert_eq!(r, json!({"mode": "each_step", "previous": "each_feature"}));
    assert_eq!(s.validation(), Validation::EachStep);
    s.execute("file.new", &json!({})).unwrap();
    s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
    s.execute("timeline.compute_all", &json!({})).unwrap();
    assert_eq!(s.execute("diagnostics.validation", &json!({"mode": "off"})).unwrap()["previous"], json!("each_step"));
    assert_eq!(s.validation(), Validation::Off);
    // A refused mode changes nothing.
    assert!(s.execute("diagnostics.validation", &json!({"mode": "never"})).is_err());
    assert_eq!(s.validation(), Validation::Off);
}

/// A feature whose body SMLib's check refuses fails under each_feature as under each_step (once
/// at its end instead of at the step), with the check's reason; off keeps the body. The bar's
/// top edge rounded into the boss's round (kernel case
/// fillets/1207-fillet-bar-top-edge-after-boss-arc-round-r0-5, refused by the check on this
/// SMLib); the same features without the last round build under every mode, to the same volume.
#[test]
fn a_body_the_check_refuses_still_fails_its_feature_once_per_feature() {
    use solidcraft_kernel::Validation;
    let build = |mode: Validation, last: bool| {
        let mut s = Session::default();
        s.set_validation(mode);
        s.execute("solid.box", &json!({"length": 100, "width": 30, "height": 10, "corner": [0, -15, 0], "body_name": "Arm"})).unwrap();
        s.execute("solid.cylinder", &json!({"diameter": 40, "height": 20, "base": [0, 0, 0], "operation": "join", "targets": ["Arm"]})).unwrap();
        s.execute("solid.fillet", &json!({"edges": [[16, 12, 10]], "radius": 2})).unwrap();
        let r = if last { s.execute("solid.fillet", &json!({"edges": [[60, -15, 10]], "radius": 0.5})).map(|_| ()) } else { Ok(()) };
        let volume = s.execute("inspect.measure", &json!({})).ok().and_then(|m| m["total"]["volume_mm3"].as_f64());
        (r.map_err(|e| e.to_string()), volume)
    };
    let each_step = build(Validation::EachStep, false);
    for mode in [Validation::EachFeature, Validation::Off] {
        let (r, volume) = build(mode, false);
        assert!(r.is_ok() && each_step.0.is_ok(), "{mode:?}: {r:?}");
        assert!(volume.zip(each_step.1).is_some_and(|(v, w)| (v - w).abs() <= 1e-9 * w), "{mode:?}: {volume:?} vs {:?}", each_step.1);
    }
    let (step, _) = build(Validation::EachStep, true);
    let (feature, _) = build(Validation::EachFeature, true);
    assert_eq!(step.is_err(), feature.is_err(), "each_step {step:?}, each_feature {feature:?}");
    if let Err(e) = &feature {
        assert!(e.contains("SMLib's check refuses the result"), "{e}");
        assert!(step.as_ref().err().is_some_and(|e| e.contains("SMLib's check refuses the result")), "{step:?}");
        let (off, _) = build(Validation::Off, true);
        assert!(off.is_ok(), "off keeps the body the check refuses: {off:?}");
    }
}
