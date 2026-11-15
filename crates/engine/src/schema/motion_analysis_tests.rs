use std::collections::BTreeSet;

use serde_json::{Value, json};

use super::{InterferenceParams, MotionInterference, MotionPlot, PlotParams};
use crate::Session;
use crate::schema::tests::{assert_keys, conforms};
use crate::schema::{ParamCheck, input_schema_for, output_schema_for};

/// Cube B (x 20…30) slides on joint `Rail` toward cube A (x 0…10), as in
/// `cmd/motion_analysis.rs`'s tests: study `Push` takes it from 10 mm away until it touches A,
/// and study `Through` 5 mm into A.
fn rail() -> Session {
    let mut s = Session::default();
    let run = |s: &mut Session, id: &str, p: Value| {
        s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
    };
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    run(&mut s, "component.activate", json!({"component": "root"}));
    let a = s.doc.find_component("A").and_then(|c| s.doc.occurrence_of(c)).map(|o| o.id).unwrap_or(0);
    run(&mut s, "occurrence.ground", json!({"occurrence": a, "grounded": true}));
    run(&mut s, "joint.create", json!({"type": "slider", "a": {"face": [10, 5, 5]}, "b": {"face": [20, 5, 5]}, "name": "Rail"}));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 10}));
    run(&mut s, "motion.study", json!({"name": "Push", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, 0]]}]}));
    run(&mut s, "motion.study", json!({"name": "Through", "steps": 15, "keys": [{"joint": "Rail", "points": [[0, 10], [15, -5]]}]}));
    s
}

#[test]
fn schemas_and_the_key_check_agree() {
    assert_keys(input_schema_for::<PlotParams>(), &["duration", "joints", "occurrence", "point", "samples", "study"]);
    assert_keys(
        input_schema_for::<InterferenceParams>(),
        &["against", "budget_s", "contact_sets", "exclude_pairs", "occurrences", "samples", "study"],
    );
    for schema in [input_schema_for::<PlotParams>(), input_schema_for::<InterferenceParams>()] {
        assert_eq!(schema.as_value()["required"], json!(["study"]));
    }
    let plot = input_schema_for::<PlotParams>();
    assert_eq!(plot.as_value()["dependentRequired"], json!({"occurrence": ["point"], "point": ["occurrence"]}));
}

/// #12852: calls that fail today get the same message in strict: the handlers' for a study,
/// `samples`, `duration`, `joints`, the followed occurrence and point, and `budget_s` of the wrong
/// shape (`study`, `occurrence` and `point` left out included: the schema requires them, and
/// the types leave them to the handlers), the part lists' (`occurrences` and `against`), and the
/// handlers' own for what the design lacks. They need the rail's studies, so the shared helper
/// (on a sketch) doesn't fit.
#[test]
fn strict_keeps_todays_errors() {
    let many_joints = vec![json!("Rail"); 257];
    let long_path = vec![json!(1); 65];
    let too_many_parts = vec![json!("A"); 10_001];
    for (id, p) in [
        ("motion.plot", json!({})),
        ("motion.plot", json!({"study": null})),
        ("motion.plot", json!({"study": [1]})),
        ("motion.plot", json!({"study": "Nope"})),
        ("motion.plot", json!({"study": "Push", "samples": 2})),
        ("motion.plot", json!({"study": "Push", "samples": "10"})),
        ("motion.plot", json!({"study": "Push", "samples": null})),
        ("motion.plot", json!({"study": "Push", "duration": 0})),
        ("motion.plot", json!({"study": "Push", "duration": "1"})),
        ("motion.plot", json!({"study": "Push", "joints": "Rail"})),
        ("motion.plot", json!({"study": "Push", "joints": many_joints})),
        ("motion.plot", json!({"study": "Push", "joints": [true]})),
        ("motion.plot", json!({"study": "Push", "joints": ["Nope"]})),
        ("motion.plot", json!({"study": "Push", "occurrence": "B"})),
        ("motion.plot", json!({"study": "Push", "point": [25, 5, 5]})),
        ("motion.plot", json!({"study": "Push", "occurrence": "B", "point": null})),
        ("motion.plot", json!({"study": "Push", "occurrence": "B", "point": [25, 5]})),
        ("motion.plot", json!({"study": "Push", "occurrence": "B", "point": [2e7, 0, 0]})),
        ("motion.plot", json!({"study": "Push", "occurrence": true, "point": [25, 5, 5]})),
        ("motion.plot", json!({"study": "Push", "occurrence": [1.5], "point": [25, 5, 5]})),
        ("motion.plot", json!({"study": "Push", "occurrence": long_path, "point": [25, 5, 5]})),
        ("motion.plot", json!({"study": "Push", "occurrence": "Nope", "point": [25, 5, 5]})),
        ("motion.interference", json!({})),
        ("motion.interference", json!({"study": true})),
        ("motion.interference", json!({"study": "Push", "samples": 1})),
        ("motion.interference", json!({"study": "Push", "samples": 1001})),
        ("motion.interference", json!({"study": "Push", "budget_s": 0})),
        ("motion.interference", json!({"study": "Push", "budget_s": 86401})),
        ("motion.interference", json!({"study": "Push", "budget_s": "60"})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"]})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A", "B"], "contact_sets": true})),
        ("motion.interference", json!({"study": "Push", "against": ["A"]})),
        ("motion.interference", json!({"study": "Push", "contact_sets": true})),
        // The part lists: not a list, `null`, an entry that is neither an id nor a name, more
        // than 10000; `against` is read the same way, with the same messages.
        ("motion.interference", json!({"study": "Push", "occurrences": 5})),
        ("motion.interference", json!({"study": "Push", "occurrences": null})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A", true]})),
        ("motion.interference", json!({"study": "Push", "occurrences": too_many_parts.clone()})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"], "against": 5})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"], "against": [true]})),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"], "against": too_many_parts})),
    ] {
        let error = |level: ParamCheck| {
            let mut s = rail();
            s.param_check = level;
            s.execute(id, &p).err().map(|e| e.to_string())
        };
        let off = error(ParamCheck::Off);
        assert!(off.is_some(), "{id} {p} should fail today");
        assert_eq!(error(ParamCheck::Strict), off, "{id} {p}");
    }
}

/// #12852: the contracts take what the input schemas allow: each call's check finds no issue
/// exactly when it matches the schema. Left out of this on purpose: `study` left out, and
/// `occurrence` or `point` without the other (the schema requires them, and the handlers refuse
/// them with their own messages; see `strict_keeps_todays_errors`), and a point 1e7 mm or more
/// from the origin (refused by the reader; JSON Schema can't measure a length).
#[test]
fn the_checks_take_what_the_schemas_allow() {
    let schemas = [("motion.plot", input_schema_for::<PlotParams>()), ("motion.interference", input_schema_for::<InterferenceParams>())];
    let schema = |id: &str| schemas.iter().find(|(name, _)| *name == id).map(|(_, s)| s.as_value().clone()).unwrap_or_default();
    let too_many_parts = vec![json!("A"); 10_001];
    for (id, p, takes) in [
        ("motion.plot", json!({"study": "Push"}), true),
        ("motion.plot", json!({"study": 1}), true),
        ("motion.plot", json!({"study": "Push", "joints": ["Rail", 2], "samples": 10.4, "duration": 1.5}), true),
        ("motion.plot", json!({"study": "Push", "joints": null, "duration": null}), true),
        ("motion.plot", json!({"study": "Push", "occurrence": "B", "point": [25, 5, 5]}), true),
        ("motion.plot", json!({"study": "Push", "occurrence": [1, 2], "point": {"x": 1, "y": 2, "z": 3}}), true),
        ("motion.plot", json!({"study": true}), false),
        ("motion.plot", json!({"study": "Push", "samples": 2}), false),
        ("motion.plot", json!({"study": "Push", "samples": 1001}), false),
        ("motion.plot", json!({"study": "Push", "samples": null}), false),
        ("motion.plot", json!({"study": "Push", "duration": 0}), false),
        ("motion.plot", json!({"study": "Push", "duration": 2e9}), false),
        ("motion.plot", json!({"study": "Push", "joints": "Rail"}), false),
        ("motion.plot", json!({"study": "Push", "joints": [true]}), false),
        ("motion.plot", json!({"study": "Push", "occurrence": true, "point": [0, 0, 0]}), false),
        ("motion.plot", json!({"study": "Push", "occurrence": [-1], "point": [0, 0, 0]}), false),
        ("motion.plot", json!({"study": "Push", "occurrence": "B", "point": [1, 2]}), false),
        ("motion.plot", json!({"study": "Push", "occurrence": "B", "point": null}), false),
        ("motion.interference", json!({"study": "Push"}), true),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A", "B"], "samples": 2, "budget_s": 86400}), true),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"], "against": ["B"]}), true),
        ("motion.interference", json!({"study": "Push", "contact_sets": true, "budget_s": null}), true),
        ("motion.interference", json!({"study": "Push", "samples": 1}), false),
        ("motion.interference", json!({"study": "Push", "budget_s": 0}), false),
        ("motion.interference", json!({"study": "Push", "budget_s": 86401}), false),
        ("motion.interference", json!({"study": "Push", "occurrences": null}), false),
        ("motion.interference", json!({"study": "Push", "contact_sets": "yes"}), false),
        ("motion.interference", json!({"study": "Push", "occurrences": [1], "against": null}), true),
        ("motion.interference", json!({"study": "Push", "occurrences": 5}), false),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A", true]}), false),
        ("motion.interference", json!({"study": "Push", "occurrences": too_many_parts.clone()}), false),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"], "against": "B"}), false),
        ("motion.interference", json!({"study": "Push", "occurrences": ["A"], "against": too_many_parts}), false),
    ] {
        let issues = crate::schema::issues(id, &p);
        assert_eq!(issues.is_empty(), takes, "{id} {p}: {issues:?}");
        let schema = schema(id);
        assert_eq!(conforms(&schema, &schema, &p), takes, "{id} {p}");
    }
}

/// The keys of a reply.
fn keys(r: &Value) -> BTreeSet<&str> {
    r.as_object().into_iter().flatten().map(|(k, _)| k.as_str()).collect()
}

/// #12852: both commands reply as their output schemas say, in Warn without warnings, with the
/// keys they gave before: a plot per step, a plot in seconds following a point of B, a clear
/// interference check, and one where B passes into A.
#[test]
fn motion_analysis_replies_with_its_schemas() {
    let mut s = rail();
    s.param_check = ParamCheck::Warn;
    let before = s.doc.clone();
    let mut reply = |id: &str, p: Value, schema: &Value| {
        let r = s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
        assert!(conforms(schema, schema, &r), "{id}: {r} doesn't match {schema}");
        assert!(r.get("param_warnings").is_none(), "{id}: {r}");
        r
    };
    let plot = output_schema_for::<MotionPlot>();
    let plot = plot.as_value();
    let stopped = ["stopped_at", "stopped_by_loop", "stopped_by_contact"];
    let per_step = reply("motion.plot", json!({"study": "Push"}), plot);
    let plain: BTreeSet<&str> = ["study", "steps", "samples", "step", "joints"].into_iter().chain(stopped).collect();
    assert_eq!(keys(&per_step), plain, "{per_step}");
    let rail = per_step["joints"].as_array().and_then(|all| all.iter().find(|j| j["name"] == "Rail")).cloned().unwrap_or_default();
    assert_eq!((&rail["unit"], &rail["velocity_unit"]), (&json!("mm"), &json!("mm/step")), "{per_step}");
    let timed = reply("motion.plot", json!({"study": "Push", "duration": 1.5, "occurrence": "B", "point": [25, 5, 5]}), plot);
    assert_eq!(keys(&timed), plain.iter().copied().chain(["time", "point"]).collect::<BTreeSet<_>>(), "{timed}");
    assert!(timed["point"]["position"].as_array().is_some_and(|p| p.len() == 16), "{timed}");

    let interference = output_schema_for::<MotionInterference>();
    let interference = interference.as_value();
    let clear = reply("motion.interference", json!({"study": "Push"}), interference);
    let checked: BTreeSet<&str> = ["study", "steps", "clear", "collisions", "pairs", "samples", "truncated", "stopped_at", "stopped_by_loop"].into();
    assert_eq!(keys(&clear), checked, "{clear}");
    assert_eq!((&clear["clear"], &clear["collisions"]), (&json!(true), &json!([])), "{clear}");
    let through = reply("motion.interference", json!({"study": "Through", "budget_s": 600}), interference);
    assert_eq!(keys(&through), checked.iter().copied().chain(["budget_s"]).collect::<BTreeSet<_>>(), "{through}");
    let pair = &through["collisions"][0];
    assert!(through["clear"] == false && pair["volume_mm3"].as_f64().is_some_and(|v| v > 0.0), "{through}");
    let pair_keys = BTreeSet::from(["a", "b", "deepest_step", "depth_mm", "first_step", "last_step", "not_measured", "steps", "volume_mm3"]);
    assert_eq!(keys(pair), pair_keys, "{pair}");
    assert_eq!(*s.doc, *before, "the analyses leave the design as it was");
}
