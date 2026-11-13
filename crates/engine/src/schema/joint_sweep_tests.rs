use serde_json::{Value, json};

use super::{SweepParams, SweepResult};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// A 100 × 4 × 50 door hinged about Z at the origin (joint `Hinge`), with a 20 × 20 block from
/// (60, 30) in its way, as in `cmd/motion_sweep_tests.rs`.
fn door() -> Session {
    let mut s = Session::default();
    for (name, corner, size) in [("Door", [0, 0, 0], [100, 4, 50]), ("Block", [60, 30, 0], [20, 20, 50])] {
        for (id, p) in [
            ("component.activate", json!({"component": "root"})),
            ("component.create", json!({"name": name, "grounded": false})),
            ("solid.box", json!({"length": size[0], "width": size[1], "height": size[2], "corner": corner})),
        ] {
            s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
    }
    let hinge = json!({"type": "revolute", "a": {"occurrence": 0, "point": [0, 0, 0]}, "b": {"occurrence": "Door", "point": [0, 0, 0]},
        "flip": true, "name": "Hinge"});
    s.execute("joint.create", &hinge).unwrap_or_else(|e| panic!("{e}"));
    s
}

/// `p` with the door's sweep from 0 to 90 degrees added.
fn swept(mut p: Value) -> Value {
    for (key, value) in [("joint", json!("Hinge")), ("from", json!(0)), ("to", json!(90))] {
        if p.get(key).is_none() {
            p[key] = value;
        }
    }
    p
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(
        input_schema_for::<SweepParams>(),
        &[
            "against",
            "apply",
            "budget_s",
            "contact_sets",
            "exclude_pairs",
            "from",
            "index",
            "joint",
            "occurrences",
            "refine",
            "report_gaps",
            "step",
            "to",
        ],
    );
    let schema = input_schema_for::<SweepParams>();
    let required: Vec<&str> = schema.as_value()["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    assert_eq!(required, ["joint", "from", "to"]);
}

/// #12844: calls that fail today get the same message in strict: the handler's for `from` or
/// `to` left out (the schema requires them, and the type leaves them to the handler), for a joint
/// value that isn't a number in range or an expression, and for `refine` or `step` out of range,
/// and the handler's own for a joint, value or parts it can't use. It needs the door's hinge, so
/// the shared helper (on a sketch) doesn't fit.
#[test]
fn strict_keeps_todays_errors() {
    for p in [
        json!({"joint": "Hinge"}),
        json!({"joint": "Hinge", "from": 0}),
        json!({"joint": true, "from": 0, "to": 90}),
        json!({"joint": null, "from": 0, "to": 90}),
        json!({"joint": "Nope", "from": 0, "to": 90}),
        swept(json!({"from": null})),
        swept(json!({"to": true})),
        swept(json!({"to": [90]})),
        swept(json!({"from": 2e9})),
        swept(json!({"to": "Opening"})),
        swept(json!({"refine": 0})),
        swept(json!({"refine": "fine"})),
        swept(json!({"refine": 2000})),
        swept(json!({"step": -1})),
        swept(json!({"step": 1e10})),
        swept(json!({"occurrences": ["Door"]})),
        swept(json!({"occurrences": ["Door", "Block"], "contact_sets": true})),
        swept(json!({"against": ["Block"]})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = door();
            s.param_check = level;
            s.execute("joint.sweep", &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{p}");
    }
}

/// #12844: the contract takes what the input schema allows: each call's check finds no issue
/// exactly when it matches the schema. (`from` and `to` left out are the exception, on purpose:
/// see `strict_keeps_todays_errors`.)
#[test]
fn the_check_takes_what_the_schema_allows() {
    let schema = input_schema_for::<SweepParams>();
    let schema = schema.as_value();
    for (p, takes) in [
        (swept(json!({})), true),
        (json!({"joint": 1, "from": "0 deg", "to": "Opening"}), true),
        (swept(json!({"refine": null, "step": null, "index": null})), true),
        (swept(json!({"refine": 1000, "step": 1e9})), true),
        (swept(json!({"index": 0, "occurrences": ["Door", 2], "against": ["Block"], "report_gaps": true, "apply": false})), true),
        (swept(json!({"contact_sets": true})), true),
        (json!({"from": 0, "to": 90}), false),
        (swept(json!({"joint": [1]})), false),
        (swept(json!({"from": null})), false),
        (swept(json!({"to": 1e9})), false),
        (swept(json!({"to": -1e9})), false),
        (swept(json!({"refine": 0})), false),
        (swept(json!({"refine": 1001})), false),
        (swept(json!({"step": 0})), false),
        (swept(json!({"step": "1"})), false),
        (swept(json!({"index": -1})), false),
        (swept(json!({"occurrences": null})), false),
        (swept(json!({"apply": "yes"})), false),
    ] {
        let issues = crate::schema::issues("joint.sweep", &p);
        assert_eq!(issues.is_empty(), takes, "{p}: {issues:?}");
        assert_eq!(conforms(schema, schema, &p), takes, "{p}");
    }
}

/// #12844: Sweep Joint replies as its output schema says, in Warn without warnings: a sweep
/// stopped by the block, one with the gaps reported, one that reaches `to` (an expression), and
/// one applied to the design. The samples carry gaps, and the reply `min_gap`, only with
/// `report_gaps`.
#[test]
fn sweep_joint_replies_with_its_schema() {
    let schema = output_schema_for::<SweepResult>();
    let schema = schema.as_value();
    let mut s = door();
    s.param_check = ParamCheck::Warn;
    let sweep = |s: &mut Session, p: Value| {
        let r = s.execute("joint.sweep", &p).unwrap_or_else(|e| panic!("{p}: {e}"));
        assert!(conforms(schema, schema, &r), "{r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{r}");
        r
    };
    let stopped = sweep(&mut s, swept(json!({})));
    assert!(stopped["stopped_by"]["a"].is_string() && stopped["stopped_at"].is_number(), "{stopped}");
    assert_eq!((&stopped["unit"], &stopped["applied"]), (&json!("deg"), &json!(false)), "{stopped}");
    assert!(stopped.get("min_gap").is_none(), "{stopped}");
    let samples = stopped["samples"].as_array().cloned().unwrap_or_default();
    assert!(!samples.is_empty() && samples.iter().all(|x| x.as_object().is_some_and(|o| o.len() == 1 && o["value"].is_number())), "{stopped}");

    let gaps = sweep(&mut s, swept(json!({"report_gaps": true, "refine": 0.1})));
    assert!(gaps["min_gap"]["Door:1"]["mm"].is_number(), "{gaps}");
    let keys = |x: &Value| {
        let mut keys = x.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default();
        keys.sort();
        keys
    };
    assert!(gaps["samples"].as_array().is_some_and(|all| all.iter().all(|x| keys(x) == ["min_gap_mm", "pair", "parts", "value"])), "{gaps}");

    let reached = sweep(&mut s, json!({"joint": "Hinge", "from": 0, "to": "10 deg", "step": 5}));
    assert_eq!((&reached["stopped_at"], &reached["stopped_by"]), (&Value::Null, &Value::Null), "{reached}");
    assert!(reached["to"].as_f64().is_some_and(|to| (to - 10.0).abs() < 1e-9), "{reached}");

    let before = s.doc.clone();
    let applied = sweep(&mut s, json!({"joint": "Hinge", "from": 0, "to": 10, "step": 5, "apply": true}));
    assert_eq!(applied["applied"], true, "{applied}");
    assert_ne!(*s.doc, *before, "the hinge was left at 10 degrees");

    // #5963: a time budget given, one run out at once, and a sweep past its
    // sample cap all reply as the schema says.
    for p in [
        json!({"joint": "Hinge", "from": 0, "to": 90, "budget_s": 60}),
        json!({"joint": "Hinge", "from": 0, "to": "90", "budget_s": 1e-9}),
        json!({"joint": "Hinge", "from": 0, "to": 1e8, "step": 1e-3}),
    ] {
        sweep(&mut s, p);
    }
}
