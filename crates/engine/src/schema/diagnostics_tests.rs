use serde_json::json;

use super::{ProfileParams, ValidationParams};
use crate::Session;
use crate::schema::input_schema_for;
use crate::schema::tests::{assert_keys, assert_strict_keeps_todays_errors};

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<ProfileParams>(), &["enabled", "reset", "top", "trace"]);
    assert_keys(input_schema_for::<ValidationParams>(), &["mode"]);
}

#[test]
fn strict_keeps_todays_errors() {
    assert_strict_keeps_todays_errors(&[
        ("diagnostics.profile", json!({"enabled": "yes"})),
        ("diagnostics.profile", json!({"reset": 1})),
        ("diagnostics.profile", json!({"top": -1})),
        ("diagnostics.profile", json!({"top": 2.5})),
        ("diagnostics.profile", json!({"trace": 5})),
        ("diagnostics.validation", json!({"mode": "each step"})),
        ("diagnostics.validation", json!({"mode": true})),
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

/// Tests that set the process-wide kernel choice run one at a time, and put `routed` back however
/// they end (a failed assertion included).
struct KernelChoice(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

static KERNEL_TESTS: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl KernelChoice {
    fn take() -> Self {
        KernelChoice(KERNEL_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

impl Drop for KernelChoice {
    fn drop(&mut self) {
        solidcraft_kernel::kernel_route::set_choice(solidcraft_kernel::kernel_route::Choice::Routed);
    }
}

/// diagnostics.kernel lists every operation family with the table's kernel and the one it runs
/// on now, and its `choice` moves them (unless SOLIDCRAFT_KERNEL overrides it).
///
/// The choice is process-wide and the other tests in this binary build bodies meanwhile, so only
/// choices that route every family as the table does are set through the command; what the
/// others would route to is read from `Op::backend_under`, which the command's own answer follows.
#[test]
fn diagnostics_kernel_reports_and_sets_the_choice() {
    use solidcraft_kernel::kernel_route as route;
    let _choice = KernelChoice::take();
    let mut s = Session::default();
    let r = s.execute("diagnostics.kernel", &json!({})).unwrap();
    let routes = r["routes"].as_array().unwrap();
    assert_eq!(routes.len(), route::Op::ALL.len(), "{r}");
    for (row, op) in routes.iter().zip(route::Op::ALL) {
        assert_eq!(row["op"], json!(format!("{op:?}").to_lowercase()), "{r}");
        assert_eq!(row["route"], json!(op.route().id()), "{r}");
        assert!(matches!(row["backend"].as_str(), Some("smlib" | "solidcore")), "{r}");
    }
    let environment = route::overridden();
    assert_eq!(r["environment"], json!(environment), "{r}");
    // SOLIDCRAFT_KERNEL_ROUTES, as the families it moves.
    let moved: serde_json::Map<String, serde_json::Value> = route::overrides().iter().map(|(op, b)| (op.id().to_string(), json!(b.id()))).collect();
    assert_eq!(r["overrides"], serde_json::Value::Object(moved), "{r}");
    for choice in ["solidcore", "smlib", "routed"] {
        let c = route::Choice::from_id(choice).unwrap();
        for op in route::Op::ALL {
            let want = match choice {
                "routed" => op.route().id(),
                other => other,
            };
            assert_eq!(op.backend_with(c, &route::Overrides::NONE).id(), want, "{choice}: {op:?}");
        }
        if !route::Op::ALL.iter().all(|op| op.backend_under(c) == op.route()) {
            continue; // it would move other tests' bodies to another kernel
        }
        let r = s.execute("diagnostics.kernel", &json!({ "choice": choice })).unwrap();
        if environment {
            continue; // SOLIDCRAFT_KERNEL wins: the choice and the backends stay its own
        }
        assert_eq!(r["choice"], json!(choice), "{r}");
        for (row, op) in r["routes"].as_array().unwrap().iter().zip(route::Op::ALL) {
            assert_eq!(row["backend"], json!(op.backend_under(c).id()), "{choice}: {r}");
        }
    }
    let r = s.execute("diagnostics.kernel", &json!({ "choice": "routed" })).unwrap();
    assert!(environment || r["choice"] == json!("routed"), "{r}");
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
        json!("routed\0"),
        json!(5),
        json!(true),
        json!(["smlib"]),
        json!({"choice": "smlib"}),
    ] {
        match s.execute("diagnostics.kernel", &json!({ "choice": bad })) {
            Err(crate::EngineError::Internal(..)) | Ok(_) => panic!("`{}` was not refused", bad.as_str().map_or(0, str::len)),
            Err(_) => {}
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
