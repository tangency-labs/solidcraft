//! `generator.parse`, `generator.build`, `generator.check` and `generator.list`: an ordering
//! code becomes a part through the normal command path, and what cannot be built changes nothing.

use serde_json::{Value, json};

use crate::Session;

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/generators/slot-nut-20.generator.json");

fn run(s: &mut Session, id: &str, params: Value) -> Value {
    match s.execute(id, &params) {
        Ok(v) => v,
        Err(e) => panic!("{id} {params}: {e}"),
    }
}

fn body_count(s: &mut Session) -> usize {
    run(s, "inspect.measure", json!({"names_only": true}))["body_count"].as_u64().unwrap() as usize
}

#[test]
fn a_code_builds_a_component_with_the_figures_the_package_expects() {
    let mut s = Session::default();
    let r = run(&mut s, "generator.build", json!({"package": EXAMPLE, "code": "TN20-M4"}));
    assert_eq!(r["component"], "TN20-M4");
    assert_eq!(r["commands"], 2);
    assert_eq!(r["generator"]["id"], "example-slot-nut-20");
    assert_eq!(r["rows"][0]["status"], "approximate");
    let volume = r["volume_mm3"].as_f64().unwrap();
    assert!((volume - 365.788).abs() < 0.01, "{volume}");
    let size: Vec<f64> = r["bbox_mm"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    for (got, want) in size.iter().zip([10.0, 10.0, 4.0]) {
        assert!((got - want).abs() < 1e-6, "{size:?}");
    }
    assert_eq!(body_count(&mut s), 1);
    // One undo step takes the whole part away.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(body_count(&mut s), 0);
}

#[test]
fn a_package_can_be_given_inline_and_a_second_hole_is_a_repeat() {
    let text = std::fs::read_to_string(EXAMPLE).unwrap();
    let object: Value = serde_json::from_str(&text).unwrap();
    for package_json in [object, Value::String(text)] {
        let mut s = Session::default();
        let r = run(&mut s, "generator.build", json!({"package_json": package_json, "code": "tn20-m5-2"}));
        assert_eq!(r["commands"], 3);
        let volume = r["volume_mm3"].as_f64().unwrap();
        assert!((volume - 469.31).abs() < 0.05, "{volume}");
    }
}

#[test]
fn a_refused_code_is_an_error_that_changes_nothing() {
    let mut s = Session::default();
    for (code, words) in [("TN20-M3-2", "not offered"), ("TN20-M6-2", "not modelled"), ("TN20-M9", "not a TN20 ordering code"), ("", "not a TN20")] {
        let e = s.execute("generator.build", &json!({"package": EXAMPLE, "code": code})).unwrap_err().to_string();
        assert!(e.contains(words), "{code}: {e}");
    }
    assert_eq!(body_count(&mut s), 0);
}

#[test]
fn a_failing_step_leaves_the_design_as_it_was() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    let package = json!({
        "format": "solidcraft-generator/1", "id": "failing-test", "name": "Failing",
        "grammar": {"alternatives": [[{"lit": "F"}]]},
        "steps": [
            {"command": "solid.box", "params": {"length": 3, "width": 3, "height": 3}},
            {"command": "solid.box", "params": {"length": "not a length", "width": 3, "height": 3}}
        ]
    });
    let e = s.execute("generator.build", &json!({"package_json": package, "code": "F"})).unwrap_err().to_string();
    assert!(e.contains("step 2") && e.contains("solid.box"), "{e}");
    assert_eq!(body_count(&mut s), 1, "the first step was undone with the rest");
}

#[test]
fn parse_answers_a_refusal_instead_of_failing() {
    let mut s = Session::default();
    let ok = run(&mut s, "generator.parse", json!({"package": EXAMPLE, "code": "TN20-M5-2"}));
    assert_eq!(ok["ok"], true);
    assert_eq!(ok["commands"], 3);
    assert_eq!(ok["component"], "TN20-M5-2");
    let holes = ok["fields"].as_array().unwrap().iter().find(|f| f["name"] == "holes").unwrap();
    assert_eq!(holes["value"], 2);
    assert_eq!(ok["rows"][0]["source"], "S1");
    let no = run(&mut s, "generator.parse", json!({"package": EXAMPLE, "code": "TN20-M6-2"}));
    assert_eq!((no["ok"].clone(), no["reason"].clone()), (json!(false), json!("not_modelled")));
    let long = "TN20-M4".to_string() + &"9".repeat(5000);
    let no = run(&mut s, "generator.parse", json!({"package": EXAMPLE, "code": long}));
    assert_eq!(no["ok"], false);
    assert_eq!(no["reason"], "error");
    // Parsing builds nothing.
    assert_eq!(body_count(&mut s), 0);
}

#[test]
fn the_packages_own_checks_pass_in_scratch_designs() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 5, "width": 5, "height": 5}));
    let r = run(&mut s, "generator.check", json!({"package": EXAMPLE}));
    assert_eq!(r["passed"], true, "{r}");
    assert!(r["results"].as_array().unwrap().len() >= 6);
    assert_eq!(body_count(&mut s), 1, "the open design is not touched");
    // A wrong expectation is reported, not hidden.
    let mut package: Value = serde_json::from_str(&std::fs::read_to_string(EXAMPLE).unwrap()).unwrap();
    package["checks"] = json!([{"code": "TN20-M4", "volume_mm3": 1.0}, {"code": "TN20-M7", "invalid": true, "message_contains": "nonsense"}]);
    let r = run(&mut s, "generator.check", json!({"package_json": package}));
    assert_eq!(r["passed"], false);
    assert!(r["results"].as_array().unwrap().iter().filter(|row| row["ok"] == false).count() >= 2);
}

#[test]
fn hostile_parameters_are_errors_never_panics() {
    let mut s = Session::default();
    let junk = [
        json!({}),
        json!({"code": "TN20-M4"}),
        json!({"package": EXAMPLE}),
        json!({"package": EXAMPLE, "package_json": {}, "code": "x"}),
        json!({"package_json": 5, "code": "x"}),
        json!({"package_json": [], "code": "x"}),
        json!({"package_json": "not json", "code": "x"}),
        json!({"package": "/nonexistent/none.generator.json", "code": "x"}),
        json!({"package": "../../etc/passwd", "code": "x"}),
        json!({"package": "Bad Id!", "code": "x"}),
        json!({"package": "no-such-installed-id", "code": "x"}),
        json!({"package": EXAMPLE, "code": 5}),
        json!({"package": EXAMPLE, "code": "x", "extra": 1}),
        json!({"package": "/dev/null", "code": "x"}),
        json!({"package": "/", "code": "x"}),
    ];
    for id in ["generator.parse", "generator.build", "generator.check"] {
        for p in &junk {
            let _ = s.execute(id, p);
        }
    }
    assert!(s.execute("generator.build", &json!({"package": EXAMPLE, "code": "x".repeat(1 << 20)})).is_err());
    assert_eq!(body_count(&mut s), 0);
}

#[test]
fn list_reads_the_installed_folder_and_never_fails() {
    let mut s = Session::default();
    let r = run(&mut s, "generator.list", json!({}));
    assert!(r["generators"].is_array() && r["skipped"].is_array());
    assert!(s.execute("generator.list", &json!({"x": 1})).is_err());
}
